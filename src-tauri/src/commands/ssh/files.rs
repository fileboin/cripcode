//! Remote filesystem operations over SSH.
//!
//! All commands take a `server_id` (to look up the server config from
//! `ssh-servers.json`) and an absolute remote path. File listing uses
//! `find -printf` (GNU find, standard on Linux VPS). File reading uses
//! `cat`. File writing pipes content via stdin to `cat > path`.
//!
//! Reuses `FileEntry` and `FileContent` from `commands::code` so the
//! frontend can use the same types for local and remote files.

use super::config;
use super::connection::build_ssh_args;
use super::shell_quote;
use crate::commands::code::{infer_language, FileContent, FileEntry};
use crate::errors::CommandError;
use crate::types::SshServer;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Maximum remote file size to read (500 KB — same as local).
const MAX_REMOTE_FILE_SIZE: u64 = 500 * 1024;

/// SSH exec timeout for file operations (30s — file reads/writes may be slower
/// than a simple echo test).
const SSH_FILE_TIMEOUT_SECS: u64 = 30;

/// Build SSH exec args: the connection args from `build_ssh_args` + the remote
/// command appended as the last argument (the SSH CLI convention).
fn build_ssh_exec_args(server: &SshServer, remote_cmd: &str) -> Vec<String> {
    let mut args = build_ssh_args(server);
    args.push(remote_cmd.to_string());
    args
}

/// Look up a server by ID from the config file.
fn get_server(server_id: &str) -> Result<SshServer, CommandError> {
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

/// Validate a remote path: must be absolute (start with `/`), must not contain
/// `..` (prevent traversal), and for delete operations must have enough depth.
fn validate_remote_path(path: &str) -> Result<(), CommandError> {
    if !path.starts_with('/') {
        return Err(CommandError::Validation {
            field: "path".into(),
            reason: "Remote path must be an absolute path (start with /)".into(),
        });
    }
    if path.contains("..") {
        return Err(CommandError::Validation {
            field: "path".into(),
            reason: "Remote path must not contain .. (path traversal not allowed)".into(),
        });
    }
    Ok(())
}

/// Extra validation for destructive operations: the path must have at least
/// 2 segments (e.g. `/home/user` is OK, `/home` or `/` is not) to prevent
/// accidental deletion of critical system directories.
fn validate_destructive_path(path: &str) -> Result<(), CommandError> {
    validate_remote_path(path)?;
    let segments: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    if segments.len() < 2 {
        return Err(CommandError::Validation {
            field: "path".into(),
            reason: "Cannot delete a path with fewer than 2 segments (safety guardrail)".into(),
        });
    }
    Ok(())
}

/// Run an SSH exec command (no stdin) with timeout. Returns the captured
/// stdout/stderr. Uses the same `run_with_timeout` infrastructure as all
/// other CLI calls.
async fn run_ssh_exec(
    server: &SshServer,
    remote_cmd: &str,
    label: &str,
) -> Result<std::process::Output, CommandError> {
    let args = build_ssh_exec_args(server, remote_cmd);
    let mut cmd = Command::new("ssh");
    cmd.args(&args);
    crate::external_command::run_with_timeout(cmd, label, SSH_FILE_TIMEOUT_SECS).await
}

/// Run an SSH exec command with stdin data (for writing files). Pipes the
/// given content to the remote command's stdin.
async fn run_ssh_exec_with_stdin(
    server: &SshServer,
    remote_cmd: &str,
    stdin_data: &[u8],
    label: &str,
) -> Result<std::process::Output, CommandError> {
    let args = build_ssh_exec_args(server, remote_cmd);
    let mut cmd = Command::new("ssh");
    cmd.args(&args);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let mut child = cmd.spawn().map_err(|e| CommandError::Io {
        message: format!("Failed to spawn SSH command: {e}"),
    })?;

    // Write stdin in a task so it doesn't block the wait
    let stdin = child.stdin.take().ok_or_else(|| CommandError::Io {
        message: "Failed to open SSH stdin".into(),
    })?;

    // Pin the stdin so we can write then close in one async block
    let mut stdin = stdin;
    stdin
        .write_all(stdin_data)
        .await
        .map_err(|e| CommandError::Io {
            message: format!("Failed to write SSH stdin: {e}"),
        })?;
    drop(stdin); // Close stdin to signal EOF

    // Wait with timeout
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(SSH_FILE_TIMEOUT_SECS),
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
            secs: SSH_FILE_TIMEOUT_SECS,
        }),
    }
}

/// List files in a remote directory. Uses `find -printf` for a structured
/// output (type, size, name) in a single SSH call.
#[tauri::command]
#[tracing::instrument]
pub async fn list_remote_files(
    server_id: String,
    path: String,
) -> Result<Vec<FileEntry>, CommandError> {
    validate_remote_path(&path)?;
    let server = get_server(&server_id)?;

    // Use find with -printf for structured output. GNU find (standard on
    // Linux) supports -printf; the format is: type\tsize\tname\n
    let remote_cmd = format!(
        "find {} -maxdepth 1 -mindepth 1 -printf '%y\\t%s\\t%f\\n' 2>/dev/null",
        shell_quote(&path)
    );

    let label = format!("ssh list {}", server.name);
    let output = run_ssh_exec(&server, &remote_cmd, &label).await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 3 {
            continue;
        }
        let (type_char, size_str, name) = (parts[0], parts[1], parts[2]);
        let is_directory = type_char == "d";
        let size = size_str.parse::<u64>().unwrap_or(0);
        entries.push(FileEntry {
            name: name.to_string(),
            path: name.to_string(),
            is_directory,
            size,
        });
    }

    Ok(entries)
}

/// Read a remote file's content. Uses `cat` to read the file; checks for
/// null bytes to detect binary files.
#[tauri::command]
#[tracing::instrument]
pub async fn read_remote_file(
    server_id: String,
    file_path: String,
) -> Result<FileContent, CommandError> {
    validate_remote_path(&file_path)?;
    let server = get_server(&server_id)?;

    // First, get the file size via stat
    let stat_cmd = format!("stat -c '%s' {} 2>/dev/null", shell_quote(&file_path));
    let stat_label = format!("ssh stat {}", server.name);
    let stat_output = run_ssh_exec(&server, &stat_cmd, &stat_label).await?;

    let stat_stdout = String::from_utf8_lossy(&stat_output.stdout);
    let size = stat_stdout.trim().parse::<u64>().unwrap_or(0);

    if size > MAX_REMOTE_FILE_SIZE {
        return Ok(FileContent {
            content: String::new(),
            is_binary: false,
            is_truncated: true,
            size,
            language: infer_language(&file_path),
        });
    }

    // Read the file content
    let cat_cmd = format!("cat {}", shell_quote(&file_path));
    let cat_label = format!("ssh read {}", server.name);
    let output = run_ssh_exec(&server, &cat_cmd, &cat_label).await?;

    let bytes = output.stdout;

    // Check for binary content (null bytes)
    let is_binary = bytes.contains(&0u8);

    let content = if is_binary {
        String::new()
    } else {
        String::from_utf8_lossy(&bytes).to_string()
    };

    Ok(FileContent {
        content,
        is_binary,
        is_truncated: false,
        size,
        language: infer_language(&file_path),
    })
}

/// Write content to a remote file. Pipes content via stdin to `cat > path`.
#[tauri::command]
#[tracing::instrument]
pub async fn save_remote_file(
    server_id: String,
    file_path: String,
    content: String,
) -> Result<(), CommandError> {
    validate_remote_path(&file_path)?;
    let server = get_server(&server_id)?;

    // Ensure parent directory exists, then write the file
    let parent = std::path::Path::new(&file_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/".to_string());

    let remote_cmd = format!(
        "mkdir -p {} && cat > {}",
        shell_quote(&parent),
        shell_quote(&file_path)
    );
    let label = format!("ssh write {}", server.name);
    let output = run_ssh_exec_with_stdin(&server, &remote_cmd, content.as_bytes(), &label).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CommandError::Process {
            cmd: label,
            exit_code: output.status.code().unwrap_or(-1),
            stderr: stderr.trim().to_string(),
        });
    }

    Ok(())
}

/// Create a remote directory (mkdir -p).
#[tauri::command]
#[tracing::instrument]
pub async fn create_remote_directory(
    server_id: String,
    dir_path: String,
) -> Result<(), CommandError> {
    validate_remote_path(&dir_path)?;
    let server = get_server(&server_id)?;

    let remote_cmd = format!("mkdir -p {}", shell_quote(&dir_path));
    let label = format!("ssh mkdir {}", server.name);
    let output = run_ssh_exec(&server, &remote_cmd, &label).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CommandError::Process {
            cmd: label,
            exit_code: output.status.code().unwrap_or(-1),
            stderr: stderr.trim().to_string(),
        });
    }

    Ok(())
}

/// Delete a remote file or directory. Uses `rm -rf` — the destructive path
/// validation ensures the path has enough depth to avoid deleting critical
/// system directories.
#[tauri::command]
#[tracing::instrument]
pub async fn delete_remote_file(server_id: String, path: String) -> Result<(), CommandError> {
    validate_destructive_path(&path)?;
    let server = get_server(&server_id)?;

    let remote_cmd = format!("rm -rf {}", shell_quote(&path));
    let label = format!("ssh delete {}", server.name);
    let output = run_ssh_exec(&server, &remote_cmd, &label).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CommandError::Process {
            cmd: label,
            exit_code: output.status.code().unwrap_or(-1),
            stderr: stderr.trim().to_string(),
        });
    }

    Ok(())
}

/// Rename/move a remote file or directory.
#[tauri::command]
#[tracing::instrument]
pub async fn rename_remote_file(
    server_id: String,
    old_path: String,
    new_path: String,
) -> Result<(), CommandError> {
    validate_remote_path(&old_path)?;
    validate_remote_path(&new_path)?;
    let server = get_server(&server_id)?;

    let remote_cmd = format!("mv {} {}", shell_quote(&old_path), shell_quote(&new_path));
    let label = format!("ssh rename {}", server.name);
    let output = run_ssh_exec(&server, &remote_cmd, &label).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CommandError::Process {
            cmd: label,
            exit_code: output.status.code().unwrap_or(-1),
            stderr: stderr.trim().to_string(),
        });
    }

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

    #[test]
    fn validate_destructive_path_rejects_root() {
        assert!(validate_destructive_path("/").is_err());
    }

    #[test]
    fn validate_destructive_path_rejects_single_segment() {
        assert!(validate_destructive_path("/home").is_err());
    }

    #[test]
    fn validate_destructive_path_accepts_deep_path() {
        assert!(validate_destructive_path("/home/user/myproject/src").is_ok());
    }

    #[test]
    fn build_ssh_exec_args_appends_remote_cmd() {
        let server = SshServer {
            id: "test".into(),
            name: "Test".into(),
            host: "example.com".into(),
            port: Some(22),
            username: "deploy".into(),
            key_path: None,
            created_at: 0,
            last_connected_at: None,
        };
        let args = build_ssh_exec_args(&server, "ls /");
        assert_eq!(args[args.len() - 1], "ls /");
        assert!(args.contains(&"deploy@example.com".to_string()));
    }
}
