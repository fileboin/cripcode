//! # SSH Server Commands
//!
//! SSH server configuration CRUD and connection state management.
//! Server configs are persisted to `~/CripCode/.cripcode/ssh-servers.json`;
//! connection state is in-memory only.
//!
//! See `docs/ssh-architecture.md` for the full architecture.

mod ai_provider;
mod config;
mod connection;
mod files;
mod ollama;
mod remote_agent;
mod remote_build;
mod remote_dev_server;
mod remote_git;
mod remote_preview;
mod remote_projects;

pub use ai_provider::*;
pub use config::*;
pub use connection::*;
pub use files::*;
pub use ollama::*;
pub use remote_agent::*;
pub use remote_build::*;
pub use remote_dev_server::*;
pub use remote_git::*;
pub use remote_preview::*;
pub use remote_projects::*;

/// Quote a value for safe interpolation into a POSIX shell command executed
/// on the remote host. Everything inside single quotes is literal to the
/// shell; an embedded `'` is replaced with the standard close/escape/reopen
/// sequence `'\''`. Without this, a path like `/tmp/x; rm -rf /` (accepted by
/// `validate_remote_path`, which only blocks `..` and relative paths) would
/// execute as two commands on the VPS.
pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod shell_quote_tests {
    use super::shell_quote;

    #[test]
    fn plain_path_is_single_quoted() {
        assert_eq!(shell_quote("/home/user/app"), "'/home/user/app'");
    }

    #[test]
    fn semicolon_cannot_start_a_second_command() {
        assert_eq!(shell_quote("/tmp/x; rm -rf /"), "'/tmp/x; rm -rf /'");
    }

    #[test]
    fn and_chain_stays_inert() {
        assert_eq!(shell_quote("/tmp/a && rm -rf /"), "'/tmp/a && rm -rf /'");
    }

    #[test]
    fn command_substitution_stays_inert() {
        assert_eq!(shell_quote("/tmp/$(rm -rf /)"), "'/tmp/$(rm -rf /)'");
    }

    #[test]
    fn backtick_substitution_stays_inert() {
        assert_eq!(shell_quote("/tmp/`rm -rf /`"), "'/tmp/`rm -rf /`'");
    }

    #[test]
    fn dollar_and_double_quote_stay_inert() {
        assert_eq!(shell_quote("/tmp/$HOME \"file\""), "'/tmp/$HOME \"file\"'");
    }

    #[test]
    fn embedded_single_quote_is_safely_escaped() {
        assert_eq!(shell_quote("/tmp/it's"), "'/tmp/it'\\''s'");
        // A leading `'` used to close the opening quote and break out:
        // the escaped form keeps the `; rm -rf /` literal inside one word.
        assert_eq!(shell_quote("'; rm -rf /"), "''\\''; rm -rf /'");
    }
}
