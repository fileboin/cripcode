//! # SSH Server Commands
//!
//! SSH server configuration CRUD and connection state management.
//! Server configs are persisted to `~/CripCode/.cripcode/ssh-servers.json`;
//! connection state is in-memory only.
//!
//! See `docs/ssh-architecture.md` for the full architecture.

use crate::types::SshServer;

mod ai_provider;
mod config;
mod connection;
mod files;
mod host_key;
mod ollama;
mod remote_agent;
mod remote_build;
mod remote_dev_server;
mod remote_git;
mod remote_preview;
mod remote_projects;
/// Public so the `ssh-askpass` helper binary can reuse the keystore wiring.
pub mod secrets;

pub use ai_provider::*;
pub use config::*;
pub use connection::*;
pub use files::*;
pub use host_key::*;
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

/// Quote a complete shell program as the single argument to `bash -c`.
/// Shell operators inside the program remain executable by that inner shell;
/// they cannot alter the outer SSH command structure.
pub(crate) fn shell_program_arg(program: &str) -> String {
    shell_quote(program)
}

/// Build SSH argv for a remote command. The command is the final argument;
/// unlike `build_ssh_args`, this does not append the connection-test command.
pub(crate) fn build_remote_ssh_args(server: &SshServer, remote_command: &str) -> Vec<String> {
    let mut args = connection::build_ssh_connection_args(server);
    args.push(remote_command.to_string());
    args
}

#[cfg(test)]
mod shell_quote_tests {
    use super::{build_remote_ssh_args, shell_program_arg, shell_quote};
    use crate::types::SshServer;

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

    #[test]
    fn shell_program_preserves_inner_shell_operators() {
        let program = "printf '%s' 'safe' && printf '%s' \"$HOME\"";
        assert_eq!(shell_program_arg(program), shell_quote(program));
        assert!(shell_program_arg(program).contains("&&"));
    }

    #[test]
    fn remote_ssh_args_send_only_the_requested_remote_command() {
        let server = SshServer {
            id: "test".into(),
            name: "Test".into(),
            host: "example.com".into(),
            port: Some(22),
            username: "deploy".into(),
            key_path: None,
            auth_type: crate::types::AuthType::Key,
            created_at: 0,
            last_connected_at: None,
        };
        let args = build_remote_ssh_args(&server, "printf '%s' 'remote'");
        assert_eq!(
            args.last().map(String::as_str),
            Some("printf '%s' 'remote'")
        );
        assert!(!args.contains(&"__cripcode_ssh_ok__".to_string()));
    }
}
