//! Remote command transport seam.
//!
//! Both platforms expose the same `std::process::Output` surface so callers
//! (Files, agent check, OpenCode auth) parse results identically:
//! - Desktop shells out to the OpenSSH client exactly as before.
//! - Android runs the command over the in-process russh transport
//!   (`transport_russh::exec`) — there is no `ssh` binary to spawn.
//!
//! The Android path always verifies the host key against the app-private
//! `known_hosts` (the same strict handler the PTY transport uses); the
//! desktop-only `accept-new` TOFU policy is never used there.

use crate::errors::CommandError;
use crate::types::SshServer;

/// Run a remote command without stdin.
pub(crate) async fn run(
    server: &SshServer,
    remote_cmd: &str,
    strict: bool,
    label: &str,
    timeout_secs: u64,
) -> Result<std::process::Output, CommandError> {
    #[cfg(not(target_os = "android"))]
    {
        let args = if strict {
            super::build_remote_ssh_args_strict(server, remote_cmd)
        } else {
            super::build_remote_ssh_args(server, remote_cmd)
        };
        let mut cmd = tokio::process::Command::new("ssh");
        cmd.args(&args);
        super::connection::apply_ssh_auth_env(&mut cmd, server)?;
        crate::external_command::run_with_timeout(cmd, label, timeout_secs).await
    }
    #[cfg(target_os = "android")]
    {
        // The strict flag is desktop policy split; Android has a single
        // host-key rule (verified known_hosts entry or fail closed).
        let _ = strict;
        super::transport_russh::exec(server, remote_cmd, None, label, timeout_secs).await
    }
}

/// Run a remote command with `stdin_data` piped to it (file writes, auth
/// payloads). The stdin content never enters argv or the environment.
pub(crate) async fn run_with_stdin(
    server: &SshServer,
    remote_cmd: &str,
    strict: bool,
    stdin_data: &[u8],
    label: &str,
    timeout_secs: u64,
) -> Result<std::process::Output, CommandError> {
    #[cfg(not(target_os = "android"))]
    {
        let args = if strict {
            super::build_remote_ssh_args_strict(server, remote_cmd)
        } else {
            super::build_remote_ssh_args(server, remote_cmd)
        };
        let mut cmd = tokio::process::Command::new("ssh");
        cmd.args(&args);
        super::connection::apply_ssh_auth_env(&mut cmd, server)?;
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| CommandError::Io {
            message: format!("Failed to spawn SSH command: {e}"),
        })?;
        let stdin = child.stdin.take().ok_or_else(|| CommandError::Io {
            message: "Failed to open SSH stdin".into(),
        })?;
        let mut stdin = stdin;
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(stdin_data)
            .await
            .map_err(|e| CommandError::Io {
                message: format!("Failed to write SSH stdin: {e}"),
            })?;
        drop(stdin); // Close stdin to signal EOF

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            child.wait_with_output(),
        )
        .await;
        match result {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(CommandError::Io {
                message: format!("SSH command failed: {e}"),
            }),
            Err(_) => Err(CommandError::Timeout {
                cmd: label.to_string(),
                secs: timeout_secs,
            }),
        }
    }
    #[cfg(target_os = "android")]
    {
        let _ = strict;
        super::transport_russh::exec(server, remote_cmd, Some(stdin_data), label, timeout_secs)
            .await
    }
}
