//! Remote dev server management over SSH.
//!
//! Manages a dev server running on a remote VPS. The server is started via
//! SSH exec in background mode (`nohup ... &`), and its status/logs are
//! queried via subsequent SSH calls. This is NOT a deploy system — it's the
//! same dev-server lifecycle as the local flow, just executed remotely.
//!
//! Supported operations: start, stop, restart, status, logs, port detection.

use super::config;
use super::{build_remote_ssh_args, shell_program_arg, shell_quote};
use crate::errors::CommandError;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Timeout for SSH dev server commands.
const SSH_DEV_TIMEOUT_SECS: u64 = 30;

/// PID file path on the VPS — stores the dev server process PID so we can
/// stop it later. One per project path.
fn pid_file_path(remote_path: &str) -> String {
    // Use a hash of the path to keep the filename short and unique
    let hash = simple_hash(remote_path);
    format!("/tmp/cripcode-devserver-{}.pid", hash)
}

/// Log file path on the VPS — captures stdout+stderr of the dev server.
fn log_file_path(remote_path: &str) -> String {
    let hash = simple_hash(remote_path);
    format!("/tmp/cripcode-devserver-{}.log", hash)
}

/// Simple string hash (FNV-1a inspired) for generating unique filenames.
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Look up a server by ID.
fn get_server(server_id: &str) -> Result<crate::types::SshServer, CommandError> {
    let config = config::load_config_pub().map_err(CommandError::from)?;
    config
        .servers
        .into_iter()
        .find(|s| s.id == server_id)
        .ok_or_else(|| CommandError::Validation {
            field: "server_id".into(),
            reason: format!("No SSH server found with id `{server_id}`"),
        })
}

/// Validate a remote path.
fn validate_path(path: &str) -> Result<(), CommandError> {
    if !path.starts_with('/') {
        return Err(CommandError::Validation {
            field: "remotePath".into(),
            reason: "Remote path must be absolute".into(),
        });
    }
    if path.contains("..") {
        return Err(CommandError::Validation {
            field: "remotePath".into(),
            reason: "Remote path must not contain ..".into(),
        });
    }
    Ok(())
}

/// Run a command on the remote VPS via SSH.
async fn run_remote(
    server_id: &str,
    remote_cmd: &str,
    label: &str,
) -> Result<std::process::Output, CommandError> {
    let server = get_server(server_id)?;
    let args = build_remote_ssh_args(&server, remote_cmd);
    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args(&args);
    super::connection::apply_ssh_auth_env(&mut cmd, &server)?;
    crate::external_command::run_with_timeout(cmd, label, SSH_DEV_TIMEOUT_SECS).await
}

fn build_start_command(
    remote_path: &str,
    command: &str,
    port: Option<u16>,
    log_file: &str,
    pid_file: &str,
) -> String {
    let port_env = port.map(|p| format!("PORT={} ", p)).unwrap_or_default();
    let shell_program = format!("{}{}", port_env, command);
    format!(
        "cd {} && nohup bash -c {} > {} 2>&1 & echo $! > {}",
        shell_quote(remote_path),
        shell_program_arg(&shell_program),
        shell_quote(log_file),
        shell_quote(pid_file)
    )
}

/// Dev server status.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDevServerStatus {
    /// Whether the dev server process is running on the VPS.
    pub running: bool,
    /// The PID of the dev server process (if running).
    pub pid: Option<u32>,
    /// The port the dev server is listening on (if detectable).
    pub port: Option<u16>,
    /// Number of lines of logs available.
    pub log_lines: u32,
    /// Error message if the status check failed.
    pub error: Option<String>,
}

/// In-memory cache of dev server status per `(server_id, remote_path)`.
/// Updated on start/stop/status calls so the frontend can poll cheaply.
static STATUS_CACHE: LazyLock<Mutex<HashMap<String, RemoteDevServerStatus>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn cache_key(server_id: &str, remote_path: &str) -> String {
    format!("{}:{}", server_id, remote_path)
}

/// Start a dev server on the remote VPS.
///
/// Runs the given command (e.g. `npm run dev`) in the project directory
/// using `nohup` to keep it alive after the SSH session closes. The PID is
/// saved to a file so we can stop it later. Output goes to a log file.
#[tauri::command]
#[tracing::instrument]
pub async fn start_remote_dev_server(
    server_id: String,
    remote_path: String,
    command: String,
    port: Option<u16>,
) -> Result<(), CommandError> {
    validate_path(&remote_path)?;

    // Check if already running
    let status = check_remote_dev_server_status_impl(&server_id, &remote_path).await;
    if status.running {
        return Err(CommandError::Expected {
            message: "Dev server is already running. Stop it first.".into(),
        });
    }

    let pid_file = pid_file_path(&remote_path);
    let log_file = log_file_path(&remote_path);

    // The command remains a complete shell program for the inner bash; only
    // its outer SSH argument boundary is quoted.
    let remote_cmd = build_start_command(&remote_path, &command, port, &log_file, &pid_file);

    let label = format!("ssh dev-start {}", remote_path);
    let output = run_remote(&server_id, &remote_cmd, &label).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CommandError::Process {
            cmd: label,
            exit_code: output.status.code().unwrap_or(-1),
            stderr: stderr.trim().to_string(),
        });
    }

    // Update status cache
    let pid = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .ok();

    let status = RemoteDevServerStatus {
        running: true,
        pid,
        port,
        log_lines: 0,
        error: None,
    };
    if let Ok(mut cache) = STATUS_CACHE.lock() {
        cache.insert(cache_key(&server_id, &remote_path), status);
    }

    Ok(())
}

/// Stop the dev server on the remote VPS.
///
/// Reads the PID file and sends SIGTERM (then SIGKILL after 5s if needed).
#[tauri::command]
#[tracing::instrument]
pub async fn stop_remote_dev_server(
    server_id: String,
    remote_path: String,
) -> Result<(), CommandError> {
    validate_path(&remote_path)?;

    let pid_file = pid_file_path(&remote_path);

    let remote_cmd = format!(
        "if [ -f {} ]; then \
            PID=$(cat {}); \
            kill $PID 2>/dev/null; \
            sleep 2; \
            kill -9 $PID 2>/dev/null; \
            rm -f {}; \
            echo 'stopped'; \
         else echo 'not running'; fi",
        shell_quote(&pid_file),
        shell_quote(&pid_file),
        shell_quote(&pid_file)
    );

    let label = format!("ssh dev-stop {}", remote_path);
    let _ = run_remote(&server_id, &remote_cmd, &label).await?;

    // Update status cache
    if let Ok(mut cache) = STATUS_CACHE.lock() {
        cache.insert(
            cache_key(&server_id, &remote_path),
            RemoteDevServerStatus {
                running: false,
                pid: None,
                port: None,
                log_lines: 0,
                error: None,
            },
        );
    }

    Ok(())
}

/// Restart the dev server (stop + start with the same command).
/// The frontend should call stop then start — this is a convenience wrapper
/// that does both. The `command` and `port` must be re-supplied because the
/// backend doesn't persist them (the frontend owns dev-server config).
#[tauri::command]
#[tracing::instrument]
pub async fn restart_remote_dev_server(
    server_id: String,
    remote_path: String,
    command: String,
    port: Option<u16>,
) -> Result<(), CommandError> {
    // Best-effort stop — ignore "not running" errors
    let _ = stop_remote_dev_server(server_id.clone(), remote_path.clone()).await;

    // Wait a moment for the port to free up
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    start_remote_dev_server(server_id, remote_path, command, port).await
}

/// Internal status check — used by both the public command and start/stop.
async fn check_remote_dev_server_status_impl(
    server_id: &str,
    remote_path: &str,
) -> RemoteDevServerStatus {
    let pid_file = pid_file_path(remote_path);
    let log_file = log_file_path(remote_path);

    // Check if the PID file exists and the process is still alive
    let remote_cmd = format!(
        "if [ -f {} ]; then \
            PID=$(cat {}); \
            if kill -0 $PID 2>/dev/null; then \
                echo \"RUNNING $PID\"; \
                LOG_LINES=$(wc -l < {} 2>/dev/null || echo 0); \
                echo \"LOGS $LOG_LINES\"; \
            else \
                rm -f {}; \
                echo 'DEAD'; \
            fi; \
         else echo 'NOTSTARTED'; fi",
        shell_quote(&pid_file),
        shell_quote(&pid_file),
        shell_quote(&log_file),
        shell_quote(&pid_file)
    );

    let label = format!("ssh dev-status {}", remote_path);
    match run_remote(server_id, &remote_cmd, &label).await {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let trimmed = stdout.trim();

            if trimmed.starts_with("RUNNING") {
                let pid = trimmed
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|s| s.parse::<u32>().ok());
                let log_lines = trimmed
                    .lines()
                    .nth(1)
                    .and_then(|l| l.strip_prefix("LOGS "))
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    .unwrap_or(0);

                RemoteDevServerStatus {
                    running: true,
                    pid,
                    port: None,
                    log_lines,
                    error: None,
                }
            } else if trimmed == "DEAD" || trimmed == "NOTSTARTED" {
                RemoteDevServerStatus {
                    running: false,
                    pid: None,
                    port: None,
                    log_lines: 0,
                    error: None,
                }
            } else {
                RemoteDevServerStatus {
                    running: false,
                    pid: None,
                    port: None,
                    log_lines: 0,
                    error: Some(format!("Unexpected status output: {}", trimmed)),
                }
            }
        }
        Err(e) => RemoteDevServerStatus {
            running: false,
            pid: None,
            port: None,
            log_lines: 0,
            error: Some(e.to_string()),
        },
    }
}

/// Check the status of the dev server on the remote VPS.
#[tauri::command]
#[tracing::instrument]
pub async fn get_remote_dev_server_status(
    server_id: String,
    remote_path: String,
) -> Result<RemoteDevServerStatus, CommandError> {
    validate_path(&remote_path)?;
    let status = check_remote_dev_server_status_impl(&server_id, &remote_path).await;

    // Update cache
    if let Ok(mut cache) = STATUS_CACHE.lock() {
        cache.insert(cache_key(&server_id, &remote_path), status.clone());
    }

    Ok(status)
}

/// Get recent dev server logs from the VPS.
///
/// Returns the last N lines of the log file. Default: 100 lines.
#[tauri::command]
#[tracing::instrument]
pub async fn get_remote_dev_server_logs(
    server_id: String,
    remote_path: String,
    lines: Option<u32>,
) -> Result<String, CommandError> {
    validate_path(&remote_path)?;
    let log_file = log_file_path(&remote_path);
    let n = lines.unwrap_or(100);

    let remote_cmd = format!(
        "tail -n {} {} 2>/dev/null || echo '(no logs yet)'",
        n,
        shell_quote(&log_file)
    );
    let label = format!("ssh dev-logs {}", remote_path);
    let output = run_remote(&server_id, &remote_cmd, &label).await?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_path_rejects_relative() {
        assert!(validate_path("relative/path").is_err());
    }

    #[test]
    fn validate_path_rejects_traversal() {
        assert!(validate_path("/home/user/../etc").is_err());
    }

    #[test]
    fn validate_path_accepts_absolute() {
        assert!(validate_path("/home/user/myproject").is_ok());
    }

    #[test]
    fn simple_hash_is_deterministic() {
        assert_eq!(simple_hash("/home/user/app"), simple_hash("/home/user/app"));
    }

    #[test]
    fn simple_hash_differs_for_different_paths() {
        assert_ne!(
            simple_hash("/home/user/app"),
            simple_hash("/home/user/other")
        );
    }

    #[test]
    fn pid_file_path_includes_hash() {
        let path = pid_file_path("/home/user/app");
        assert!(path.starts_with("/tmp/cripcode-devserver-"));
        assert!(path.ends_with(".pid"));
    }

    #[test]
    fn log_file_path_includes_hash() {
        let path = log_file_path("/home/user/app");
        assert!(path.starts_with("/tmp/cripcode-devserver-"));
        assert!(path.ends_with(".log"));
    }

    #[test]
    fn start_command_quotes_path_and_preserves_shell_program() {
        let command = build_start_command(
            "/tmp/project; touch /tmp/injected",
            "printf '%s' 'safe' && echo \"$HOME\"",
            Some(3000),
            "/tmp/cripcode-devserver-1.log",
            "/tmp/cripcode-devserver-1.pid",
        );
        assert_eq!(
            command,
            "cd '/tmp/project; touch /tmp/injected' && nohup bash -c 'PORT=3000 printf '\\''%s'\\'' '\\''safe'\\'' && echo \"$HOME\"' > '/tmp/cripcode-devserver-1.log' 2>&1 & echo $! > '/tmp/cripcode-devserver-1.pid'"
        );
    }
}
