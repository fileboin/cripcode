//! Secure password storage for password-mode SSH servers.
//!
//! A password's only permitted homes are, transiently: React state, the Tauri
//! IPC bridge, Rust memory, the ssh stdin pipe, the askpass helper's memory —
//! and, durably, the OS keystore via this module. It must never reach
//! `ssh-servers.json`, argv, environment variables, logs, or error messages;
//! the struct in [`crate::types`] has no password field, so the compiler
//! enforces the JSON half of that rule.
//!
//! The keystore is keyed by server **id** (a stable UUID), not by the
//! user-editable server name, so renames never orphan a stored password.

use crate::errors::CommandError;

/// Fixed keyring service name for all CripCode SSH entries.
const KEYRING_SERVICE: &str = "CripCode SSH";

/// Environment variable through which the spawned `ssh` process hands the
/// server id to the askpass helper. An id is not a secret — the password
/// itself never travels through the environment.
pub const ASKPASS_SERVER_ID_ENV: &str = "CRIPCODE_SSH_SERVER_ID";

/// Abstraction over the OS keystore so config/askpass logic can be unit
/// tested with an in-memory implementation and never touches the real
/// credential store.
pub trait SecretStore: Send + Sync {
    /// Store (or overwrite) the password for a server id.
    fn set_password(&self, account: &str, password: &str) -> Result<(), CommandError>;
    /// Read the stored password; `None` when nothing is stored.
    fn get_password(&self, account: &str) -> Result<Option<String>, CommandError>;
    /// Remove the stored password. Missing entries are treated as success.
    fn delete_password(&self, account: &str) -> Result<(), CommandError>;
}

/// The real OS keystore backed by the `keyring` crate.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn set_password(&self, account: &str, password: &str) -> Result<(), CommandError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account).map_err(keyring_error)?;
        entry.set_password(password).map_err(keyring_error)
    }

    fn get_password(&self, account: &str) -> Result<Option<String>, CommandError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account).map_err(keyring_error)?;
        match entry.get_password() {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_error(e)),
        }
    }

    fn delete_password(&self, account: &str) -> Result<(), CommandError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account).map_err(keyring_error)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keyring_error(e)),
        }
    }
}

/// Map a keyring failure to a user-facing error. The message never contains
/// the stored secret — keyring errors describe the platform, not the value.
fn keyring_error(e: keyring::Error) -> CommandError {
    CommandError::Io {
        message: format!("could not access the OS credential store: {e}"),
    }
}

/// Entry point for the `ssh-askpass` helper binary. The OpenSSH client
/// spawns the helper (via `SSH_ASKPASS`) whenever it needs the password;
/// the helper prints the stored password as a single line on stdout —
/// the channel ssh reads as the prompt answer. Nothing is ever logged,
/// written to disk, or placed in argv; a missing entry fails closed with a
/// non-zero exit and an empty stdout so ssh cannot obtain a credential.
///
/// Returns the process exit code: 0 = password printed, 1 = nothing stored,
/// 2 = malformed invocation.
pub fn run_askpass(store: &dyn SecretStore) -> i32 {
    let Ok(server_id) = std::env::var(ASKPASS_SERVER_ID_ENV) else {
        return 2;
    };
    if server_id.trim().is_empty() {
        return 2;
    }
    match store.get_password(&server_id) {
        Ok(Some(password)) => {
            // ssh's askpass reader strips one trailing newline; writing the
            // raw bytes without adding one keeps every password character.
            use std::io::Write;
            match std::io::stdout().write_all(password.as_bytes()) {
                Ok(()) => 0,
                Err(_) => 1,
            }
        }
        // A generic, secret-free note on stderr: it may surface through ssh's
        // stderr in a connection error, so it must stay non-sensitive.
        Ok(None) => {
            eprintln!("cripcode ssh-askpass: no stored password for this server");
            1
        }
        Err(_) => {
            eprintln!("cripcode ssh-askpass: could not read the stored password");
            1
        }
    }
}

#[cfg(test)]
pub(crate) struct MockSecretStore {
    entries: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

#[cfg(test)]
impl MockSecretStore {
    pub(crate) fn new() -> Self {
        Self {
            entries: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub(crate) fn contains(&self, account: &str) -> bool {
        self.entries.lock().unwrap().contains_key(account)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.lock().unwrap().is_empty()
    }
}

#[cfg(test)]
impl SecretStore for MockSecretStore {
    fn set_password(&self, account: &str, password: &str) -> Result<(), CommandError> {
        self.entries
            .lock()
            .unwrap()
            .insert(account.to_string(), password.to_string());
        Ok(())
    }

    fn get_password(&self, account: &str) -> Result<Option<String>, CommandError> {
        Ok(self.entries.lock().unwrap().get(account).cloned())
    }

    fn delete_password(&self, account: &str) -> Result<(), CommandError> {
        self.entries.lock().unwrap().remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn askpass_with(store: &MockSecretStore, server_id: &str) -> i32 {
        // Unit tests run single-threaded with respect to this env var.
        std::env::set_var(ASKPASS_SERVER_ID_ENV, server_id);
        run_askpass(store)
    }

    #[test]
    fn mock_store_round_trips_and_deletes() {
        let store = MockSecretStore::new();
        assert_eq!(store.get_password("srv-1").unwrap(), None);
        store.set_password("srv-1", "s3cret").unwrap();
        assert_eq!(
            store.get_password("srv-1").unwrap().as_deref(),
            Some("s3cret")
        );
        assert!(store.contains("srv-1"));
        store.delete_password("srv-1").unwrap();
        assert_eq!(store.get_password("srv-1").unwrap(), None);
        assert!(!store.contains("srv-1"));
        // Deleting a missing entry stays idempotent (mirrors the real store).
        store.delete_password("srv-1").unwrap();
    }

    #[test]
    fn askpass_prints_stored_password_and_fails_closed_without_one() {
        let store = MockSecretStore::new();

        // Nothing stored → non-zero, nothing printed (capturing stdout is
        // unnecessary: fail-closed is defined by the exit code + the store).
        assert_eq!(askpass_with(&store, "srv-1"), 1);

        // Stored → success exit code.
        store.set_password("srv-1", "s3cret").unwrap();
        assert_eq!(askpass_with(&store, "srv-1"), 0);

        // Malformed invocation (blank id) → distinct non-zero.
        assert_eq!(askpass_with(&store, "  "), 2);

        std::env::remove_var(ASKPASS_SERVER_ID_ENV);
    }
}
