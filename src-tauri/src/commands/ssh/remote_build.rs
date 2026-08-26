//! Remote build management over SSH.
//!
//! Runs a build command (e.g. `npm run build`, `pnpm build`) on a remote VPS
//! and tracks its output and exit status. Unlike a dev server, a build is a
//! one-shot process — it runs, produces output, and exits (success or failure).
//!
//! Supported operations: start, status, logs, stop.

use super::config;
use super::connection::build_ssh_args;
use crate::errors::CommandError;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Timeout for SSH build commands (builds can take longer than dev server checks).
const SSH_BUILD_TIMEOUT_SECS: u64 = 600;

/// PID/log file paths — same pattern as remote dev server but with a different prefix.
fn pid_file_path(remote_path: &str) -> String {
    let hash = simple_hash(remote_path);
    format!("/tmp/cripcode-build-{}.pid", hash)
}

fn log_file_path(remote_path: &str) -> String {
    let hash = simple_hash(remote_path);
    format!("/tmp/cripcode-build-{}.log", hash)
}

fn exit_file_path(remote_path: &str) -> String {
    let hash = simple_hash(remote_path);
    format!("/tmp/cripcode-build-{}.exit", hash)
}

/// Simple string hash (FNV-1a inspired) — same as remote_dev_server.
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

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
    timeout_secs: u64,
) -> Result<std::process::Output, CommandError> {
    let server = get_server(server_id)?;
    let mut args = build_ssh_args(&server);
    args.push(remote_cmd.to_string());
    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args(&args);
    crate::external_command::run_with_timeout(cmd, label, timeout_secs).await
}

/// Build status.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBuildStatus {
    /// Whether a build is currently running.
    pub running: bool,
    /// Exit code if the build has finished (None if still running or not started).
    pub exit_code: Option<i32>,
    /// Whether the build succeeded (exit code 0). None if not finished.
    pub success: Option<bool>,
    /// Number of log lines available.
    pub log_lines: u32,
    /// Error message if the status check failed.
    pub error: Option<String>,
}

/// In-memory cache of build status per `(server_id, remote_path)`.
static BUILD_CACHE: LazyLock<Mutex<HashMap<String, RemoteBuildStatus>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn cache_key(server_id: &str, remote_path: &str) -> String {
    format!("{}:{}", server_id, remote_path)
}

/// Start a build on the remote VPS.
///
/// Runs the given build command (e.g. `npm run build`) in the project
/// directory using `nohup`. The PID is saved to a file, output to a log
/// file, and the exit code is written to an exit file when done.
#[tauri::command]
#[tracing::instrument]
pub async fn start_remote_build(
    server_id: String,
    remote_path: String,
    command: String,
) -> Result<(), CommandError> {
    validate_path(&remote_path)?;

    // Check if a build is already running
    let status = check_remote_build_status_impl(&server_id, &remote_path).await;
    if status.running {
        return Err(CommandError::Expected {
            message: "A build is already running. Stop it first.".into(),
        });
    }

    let pid_file = pid_file_path(&remote_path);
    let log_file = log_file_path(&remote_path);
    let exit_file = exit_file_path(&remote_path);

    // Clean up any previous exit file
    let cleanup_cmd = format!("rm -f {exit_file} 2>/dev/null");
    let _ = run_remote(&server_id, &cleanup_cmd, "ssh build-cleanup", 10).await;

    // Build command: cd to project, run nohup with the build command,
    // save PID, capture output, write exit code on completion.
    let escaped_cmd = command.replace('\'', "'\\''");
    let remote_cmd = format!(
        "cd {remote_path} && nohup bash -c '{escaped_cmd}; echo $? > {exit_file}' > {log_file} 2>&1 & echo $! > {pid_file}"
    );

    let label = format!("ssh build-start {}", remote_path);
    let output = run_remote(&server_id, &remote_cmd, &label, 30).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CommandError::Process {
            cmd: label,
            exit_code: output.status.code().unwrap_or(-1),
            stderr: stderr.trim().to_string(),
        });
    }

    // Update status cache
    if let Ok(mut cache) = BUILD_CACHE.lock() {
        cache.insert(
            cache_key(&server_id, &remote_path),
            RemoteBuildStatus {
                running: true,
                exit_code: None,
                success: None,
                log_lines: 0,
                error: None,
            },
        );
    }

    Ok(())
}

/// Stop a running build on the VPS.
#[tauri::command]
#[tracing::instrument]
pub async fn stop_remote_build(server_id: String, remote_path: String) -> Result<(), CommandError> {
    validate_path(&remote_path)?;
    let pid_file = pid_file_path(&remote_path);

    let remote_cmd = format!(
        "if [ -f {pid_file} ]; then \
            PID=$(cat {pid_file}); \
            kill $PID 2>/dev/null; \
            sleep 1; \
            kill -9 $PID 2>/dev/null; \
            rm -f {pid_file}; \
            echo 'stopped'; \
         else echo 'not running'; fi"
    );

    let label = format!("ssh build-stop {}", remote_path);
    let _ = run_remote(&server_id, &remote_cmd, &label, 15).await?;

    if let Ok(mut cache) = BUILD_CACHE.lock() {
        cache.insert(
            cache_key(&server_id, &remote_path),
            RemoteBuildStatus {
                running: false,
                exit_code: None,
                success: None,
                log_lines: 0,
                error: None,
            },
        );
    }

    Ok(())
}

/// Internal status check — used by both the public command and start/stop.
async fn check_remote_build_status_impl(server_id: &str, remote_path: &str) -> RemoteBuildStatus {
    let pid_file = pid_file_path(remote_path);
    let log_file = log_file_path(remote_path);
    let exit_file = exit_file_path(remote_path);

    let remote_cmd = format!(
        "if [ -f {pid_file} ]; then \
            PID=$(cat {pid_file}); \
            if kill -0 $PID 2>/dev/null; then \
                echo 'RUNNING'; \
                LOG_LINES=$(wc -l < {log_file} 2>/dev/null || echo 0); \
                echo \"LOGS $LOG_LINES\"; \
            else \
                rm -f {pid_file}; \
                EXIT_CODE=$(cat {exit_file} 2>/dev/null || echo '?'); \
                echo \"DONE $EXIT_CODE\"; \
                LOG_LINES=$(wc -l < {log_file} 2>/dev/null || echo 0); \
                echo \"LOGS $LOG_LINES\"; \
            fi; \
         else echo 'NOTSTARTED'; fi"
    );

    let label = format!("ssh build-status {}", remote_path);
    match run_remote(server_id, &remote_cmd, &label, 10).await {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let trimmed = stdout.trim();

            if trimmed.starts_with("RUNNING") {
                let log_lines = trimmed
                    .lines()
                    .nth(1)
                    .and_then(|l| l.strip_prefix("LOGS "))
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    .unwrap_or(0);
                RemoteBuildStatus {
                    running: true,
                    exit_code: None,
                    success: None,
                    log_lines,
                    error: None,
                }
            } else if trimmed.starts_with("DONE") {
                let exit_code = trimmed
                    .lines()
                    .next()
                    .and_then(|l| l.strip_prefix("DONE "))
                    .and_then(|s| s.trim().parse::<i32>().ok());
                let log_lines = trimmed
                    .lines()
                    .nth(1)
                    .and_then(|l| l.strip_prefix("LOGS "))
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    .unwrap_or(0);
                RemoteBuildStatus {
                    running: false,
                    exit_code,
                    success: exit_code.map(|c| c == 0),
                    log_lines,
                    error: None,
                }
            } else {
                RemoteBuildStatus {
                    running: false,
                    exit_code: None,
                    success: None,
                    log_lines: 0,
                    error: None,
                }
            }
        }
        Err(e) => RemoteBuildStatus {
            running: false,
            exit_code: None,
            success: None,
            log_lines: 0,
            error: Some(e.to_string()),
        },
    }
}

/// Check the status of a build on the remote VPS.
#[tauri::command]
#[tracing::instrument]
pub async fn get_remote_build_status(
    server_id: String,
    remote_path: String,
) -> Result<RemoteBuildStatus, CommandError> {
    validate_path(&remote_path)?;
    let status = check_remote_build_status_impl(&server_id, &remote_path).await;

    if let Ok(mut cache) = BUILD_CACHE.lock() {
        cache.insert(cache_key(&server_id, &remote_path), status.clone());
    }

    Ok(status)
}

/// Get recent build logs from the VPS.
#[tauri::command]
#[tracing::instrument]
pub async fn get_remote_build_logs(
    server_id: String,
    remote_path: String,
    lines: Option<u32>,
) -> Result<String, CommandError> {
    validate_path(&remote_path)?;
    let log_file = log_file_path(&remote_path);
    let n = lines.unwrap_or(200);

    let remote_cmd = format!(
        "tail -n {} {} 2>/dev/null || echo '(no logs yet)'",
        n, log_file
    );
    let label = format!("ssh build-logs {}", remote_path);
    let output = run_remote(&server_id, &remote_cmd, &label, 15).await?;

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
    fn build_status_serializes_camel_case() {
        let status = RemoteBuildStatus {
            running: false,
            exit_code: Some(0),
            success: Some(true),
            log_lines: 42,
            error: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"exitCode\":0"));
        assert!(json.contains("\"logLines\":42"));
        assert!(json.contains("\"success\":true"));
    }

    #[test]
    fn build_status_running() {
        let status = RemoteBuildStatus {
            running: true,
            exit_code: None,
            success: None,
            log_lines: 10,
            error: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"running\":true"));
    }
}
