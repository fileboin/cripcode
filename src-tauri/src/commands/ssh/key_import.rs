//! Android SSH private-key import.
//!
//! Android has no home directory and no OpenSSH client, so an imported key
//! lives in the app-private data dir and is consumed by the russh transport
//! (`transport_russh::load_secret_key`). The key text only ever travels:
//! React state → IPC → this command → app-private file. It is never logged,
//! echoed in errors, or persisted anywhere else, and the server record keeps
//! only the absolute path.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use crate::errors::CommandError;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Directory (under the app-private data dir) holding imported keys.
pub(crate) const KEYS_DIR: &str = "keys";

/// Sanitized file name for an imported key: reuse the server id (stable
/// across re-imports of the same server) when it is a safe token, otherwise
/// a fresh UUID. Caller-supplied text can never escape the keys directory.
pub(crate) fn key_file_name(server_id: Option<&str>) -> String {
    let sanitized: String = server_id
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if sanitized.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        sanitized
    }
}

/// Write the key text to `<base>/keys/<name>` with owner-only permissions.
/// Fails closed on empty content, an unusable base directory, or any I/O
/// error. Returns the absolute path to store in the server config. Error
/// messages never contain key material.
pub(crate) fn write_private_key(
    base: &Path,
    name: &str,
    key_text: &str,
) -> Result<PathBuf, CommandError> {
    if key_text.trim().is_empty() {
        return Err(CommandError::Validation {
            field: "privateKey".into(),
            reason: "Private key must not be empty".into(),
        });
    }

    let keys_dir = base.join(KEYS_DIR);
    std::fs::create_dir_all(&keys_dir).map_err(|e| CommandError::Io {
        message: format!("Failed to create the keys directory: {e}"),
    })?;

    let path = keys_dir.join(name);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| CommandError::Io {
            message: format!("Failed to write the private key: {e}"),
        })?;
    file.write_all(key_text.as_bytes())
        .map_err(|e| CommandError::Io {
            message: format!("Failed to write the private key: {e}"),
        })?;

    // Owner-only where the platform supports it (Android is Unix). The
    // app-private dir is already inaccessible to other apps; 0600 is defense
    // in depth.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(path)
}

/// Import a pasted private key into app-private storage and return its path.
///
/// The key text is validated with the exact parser the russh transport uses
/// at connect time (`decode_secret_key`), so a key that imports is a key that
/// can authenticate — no second validator. Passphrase-protected keys are
/// rejected (fail-closed) until passphrase storage exists.
#[cfg(target_os = "android")]
#[tauri::command]
#[tracing::instrument(skip(key_text))]
pub fn import_ssh_key(key_text: String, server_id: Option<String>) -> Result<String, CommandError> {
    russh::keys::decode_secret_key(&key_text, None).map_err(|_| CommandError::Validation {
        field: "privateKey".into(),
        reason: "Not a valid unencrypted OpenSSH private key \
                 (passphrase-protected keys are not supported yet)"
            .into(),
    })?;

    let base = crate::android_paths::app_data_dir().ok_or_else(|| {
        CommandError::expected("Android app data directory is not available yet.")
    })?;
    let path = write_private_key(base, &key_file_name(server_id.as_deref()), &key_text)?;
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_base(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cripcode-key-import-{tag}-{}", Uuid::new_v4()))
    }

    #[test]
    fn file_name_reuses_a_safe_server_id() {
        assert_eq!(key_file_name(Some("abc-123_def")), "abc-123_def");
    }

    #[test]
    fn file_name_strips_path_traversal_and_separators() {
        assert_eq!(key_file_name(Some("../../etc/passwd")), "etcpasswd");
        assert_eq!(key_file_name(Some("a/b\\c")), "abc");
    }

    #[test]
    fn file_name_falls_back_to_uuid_when_unusable() {
        for input in [None, Some(""), Some("///"), Some("..")] {
            let name = key_file_name(input);
            assert!(!name.is_empty());
            assert!(
                Uuid::parse_str(&name).is_ok(),
                "expected a UUID fallback, got {name}"
            );
        }
    }

    #[test]
    fn writes_the_key_inside_the_keys_dir() {
        let base = temp_base("write");
        let path = write_private_key(&base, "server-1", "KEY-CONTENT").expect("write key");
        assert_eq!(path, base.join(KEYS_DIR).join("server-1"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "KEY-CONTENT");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn overwrites_on_reimport() {
        let base = temp_base("overwrite");
        write_private_key(&base, "server-1", "OLD").expect("first write");
        write_private_key(&base, "server-1", "NEW").expect("re-import");
        let path = base.join(KEYS_DIR).join("server-1");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "NEW");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn empty_key_fails_closed_without_writing() {
        let base = temp_base("empty");
        for input in ["", "   \n\t"] {
            let err = write_private_key(&base, "server-1", input).unwrap_err();
            assert!(
                matches!(err, CommandError::Validation { .. }),
                "empty key must be a validation failure, got {err:?}"
            );
        }
        assert!(!base.join(KEYS_DIR).exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn storage_failure_fails_closed_and_never_echoes_the_key() {
        // A base path that is an existing FILE cannot host the keys dir.
        let base = temp_base("blocked");
        std::fs::write(&base, b"not a directory").unwrap();
        let secret = "-----BEGIN OPENSSH PRIVATE KEY-----SECRET-MARKER";
        let err = write_private_key(&base, "server-1", secret).unwrap_err();
        assert!(matches!(err, CommandError::Io { .. }), "got {err:?}");
        assert!(
            !format!("{err}").contains("SECRET-MARKER"),
            "key material leaked into an error message: {err}"
        );
        assert!(
            !format!("{err:?}").contains("SECRET-MARKER"),
            "key material leaked into a debug error: {err:?}"
        );
        let _ = std::fs::remove_file(&base);
    }

    #[cfg(unix)]
    #[test]
    fn stored_key_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let base = temp_base("perms");
        let path = write_private_key(&base, "server-1", "KEY").expect("write key");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "imported key must be owner-only");
        let _ = std::fs::remove_dir_all(&base);
    }
}
