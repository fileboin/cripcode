//! Remote Git operations over SSH.
//!
//! Mirrors the essential local git commands but executes them on a remote VPS
//! via `ssh user@host "cd /path && git ..."`. All commands take a
//! `server_id` (to look up SSH config) and a `remote_path` (the project
//! directory on the VPS).
//!
//! Supported operations: status, changed files, current branch, list branches,
//! commit, pull, push, diff. Checkout/stash are deferred to a later phase
//! since they need more careful state management over SSH.

use super::config;
use super::{build_remote_ssh_args, shell_quote};
use crate::errors::CommandError;
use crate::types::{BranchInfo, ChangedFile, FileDiff};
use serde::Serialize;

/// SSH exec timeout for git operations (30s — same as file ops).
const SSH_GIT_TIMEOUT_SECS: u64 = 30;

/// Network git timeout (push/pull — may be slower).
const SSH_GIT_NET_TIMEOUT_SECS: u64 = 60;

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

/// Run a git command on the remote VPS via SSH exec.
/// Executes `cd <remote_path> && git <args>`.
async fn run_remote_git(
    server_id: &str,
    remote_path: &str,
    git_args: &str,
    timeout_secs: u64,
) -> Result<std::process::Output, CommandError> {
    let server = get_server(server_id)?;
    // remote_path is frontend-supplied: quote it so it can't terminate the
    // `cd` and inject shell operators. git_args is a fixed internal literal.
    let remote_cmd = build_remote_git_command(remote_path, git_args);
    let args = build_remote_ssh_args(&server, &remote_cmd);
    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args(&args);
    let label = format!("ssh git {} {}", server.name, git_args);
    crate::external_command::run_with_timeout(cmd, &label, timeout_secs).await
}

fn build_remote_git_command(remote_path: &str, git_args: &str) -> String {
    format!("cd {} && git {}", shell_quote(remote_path), git_args)
}

fn build_git_push_args(branch_name: &str) -> String {
    format!("push -u origin -- {}", shell_quote(branch_name))
}

fn build_git_diff_args(file_path: &str) -> String {
    format!("diff -- {}", shell_quote(file_path))
}

fn build_git_file_probe_args(file_path: &str) -> String {
    format!("ls-files --error-unmatch -- {}", shell_quote(file_path))
}

fn build_git_commit_args(message: &str) -> String {
    format!("add -A && git commit -m {}", shell_quote(message))
}

fn build_untracked_file_command(remote_path: &str, file_path: &str) -> String {
    format!(
        "cd {} && cat -- {}",
        shell_quote(remote_path),
        shell_quote(file_path)
    )
}

/// Simple branch info for remote listing. Reuses `BranchInfo` but with
/// fewer fields populated (ahead/behind not computed remotely).
#[derive(Debug, Serialize)]
pub struct RemoteBranchInfo {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
    pub last_commit_date: u64,
    pub last_commit_author: String,
}

/// Check if the remote project has uncommitted changes.
#[tauri::command]
#[tracing::instrument]
pub async fn remote_git_status(
    server_id: String,
    remote_path: String,
) -> Result<bool, CommandError> {
    validate_path(&remote_path)?;
    let output = run_remote_git(
        &server_id,
        &remote_path,
        "status --porcelain -uno",
        SSH_GIT_TIMEOUT_SECS,
    )
    .await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(!stdout.trim().is_empty())
}

/// Get the current branch name on the remote VPS.
#[tauri::command]
#[tracing::instrument]
pub async fn remote_git_current_branch(
    server_id: String,
    remote_path: String,
) -> Result<Option<String>, CommandError> {
    validate_path(&remote_path)?;
    let output = run_remote_git(
        &server_id,
        &remote_path,
        "branch --show-current",
        SSH_GIT_TIMEOUT_SECS,
    )
    .await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let branch = stdout.trim().to_string();
    if branch.is_empty() {
        Ok(None)
    } else {
        Ok(Some(branch))
    }
}

/// List branches on the remote VPS.
#[tauri::command]
#[tracing::instrument]
pub async fn remote_git_list_branches(
    server_id: String,
    remote_path: String,
) -> Result<Vec<RemoteBranchInfo>, CommandError> {
    validate_path(&remote_path)?;
    let output = run_remote_git(
        &server_id,
        &remote_path,
        &format!(
            "branch -a --format={}",
            shell_quote("%(refname:short)|%(committerdate:unix)|%(authorname)|%(HEAD)")
        ),
        SSH_GIT_TIMEOUT_SECS,
    )
    .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut branches = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 4 {
            continue;
        }
        let name = parts[0].to_string();
        let date = parts[1].parse::<u64>().unwrap_or(0) * 1000; // unix secs → ms
        let author = parts[2].to_string();
        let is_current = parts[3] == "*";

        // Skip remotes/origin/HEAD → origin/HEAD
        if name.contains("HEAD") {
            continue;
        }

        let is_remote = name.starts_with("origin/");
        let clean_name = if is_remote {
            name.strip_prefix("origin/").unwrap_or(&name).to_string()
        } else {
            name
        };

        branches.push(RemoteBranchInfo {
            name: clean_name,
            is_current,
            is_remote,
            last_commit_date: date,
            last_commit_author: author,
        });
    }

    Ok(branches)
}

/// Get changed files on the remote VPS.
#[tauri::command]
#[tracing::instrument]
pub async fn remote_git_changed_files(
    server_id: String,
    remote_path: String,
) -> Result<Vec<ChangedFile>, CommandError> {
    validate_path(&remote_path)?;
    let output = run_remote_git(
        &server_id,
        &remote_path,
        "status --porcelain -uno",
        SSH_GIT_TIMEOUT_SECS,
    )
    .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut files = Vec::new();

    for line in stdout.lines() {
        if line.len() < 3 {
            continue;
        }
        let status_char = line.chars().next().unwrap_or(' ');
        let file_path = line[3..].trim().to_string();

        let status = match status_char {
            'M' => "modified",
            'A' => "added",
            'D' => "deleted",
            'R' => "renamed",
            _ => "modified",
        };

        files.push(ChangedFile {
            path: file_path,
            status: status.to_string(),
        });
    }

    Ok(files)
}

/// Stage all changes and commit on the remote VPS.
#[tauri::command]
#[tracing::instrument]
pub async fn remote_git_commit(
    server_id: String,
    remote_path: String,
    message: String,
) -> Result<bool, CommandError> {
    validate_path(&remote_path)?;
    let message_trimmed = message.trim();
    if message_trimmed.is_empty() {
        return Err(CommandError::Validation {
            field: "message".into(),
            reason: "Commit message is required".into(),
        });
    }

    // Stage all tracked changes, then commit. The message is one Git argument;
    // the surrounding `&&` remains intentional command structure.
    let git_args = build_git_commit_args(message_trimmed);
    let output = run_remote_git(&server_id, &remote_path, &git_args, SSH_GIT_TIMEOUT_SECS).await?;

    // Exit code 0 = committed, exit code 1 = nothing to commit
    let stdout = String::from_utf8_lossy(&output.stdout);
    let nothing_to_commit = stdout.contains("nothing to commit") || stdout.contains("no changes");
    Ok(!nothing_to_commit)
}

/// Pull latest changes from remote on the VPS.
#[tauri::command]
#[tracing::instrument]
pub async fn remote_git_pull(server_id: String, remote_path: String) -> Result<(), CommandError> {
    validate_path(&remote_path)?;
    let output = run_remote_git(&server_id, &remote_path, "pull", SSH_GIT_NET_TIMEOUT_SECS).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CommandError::Process {
            cmd: "ssh git pull".into(),
            exit_code: output.status.code().unwrap_or(-1),
            stderr: stderr.trim().to_string(),
        });
    }
    Ok(())
}

/// Push the current branch to origin on the VPS.
#[tauri::command]
#[tracing::instrument]
pub async fn remote_git_push(
    server_id: String,
    remote_path: String,
    branch_name: String,
) -> Result<(), CommandError> {
    validate_path(&remote_path)?;
    let branch_trimmed = branch_name.trim();
    if branch_trimmed.is_empty() {
        return Err(CommandError::Validation {
            field: "branch_name".into(),
            reason: "Branch name is required".into(),
        });
    }

    let git_args = build_git_push_args(branch_trimmed);
    let output = run_remote_git(
        &server_id,
        &remote_path,
        &git_args,
        SSH_GIT_NET_TIMEOUT_SECS,
    )
    .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CommandError::Process {
            cmd: "ssh git push".into(),
            exit_code: output.status.code().unwrap_or(-1),
            stderr: stderr.trim().to_string(),
        });
    }
    Ok(())
}

/// Get the diff for a single file on the remote VPS.
#[tauri::command]
#[tracing::instrument]
pub async fn remote_git_diff(
    server_id: String,
    remote_path: String,
    file_path: String,
) -> Result<FileDiff, CommandError> {
    validate_path(&remote_path)?;

    // Check if the file is new (untracked)
    let is_new = !file_path.is_empty()
        && run_remote_git(
            &server_id,
            &remote_path,
            &build_git_file_probe_args(&file_path),
            SSH_GIT_TIMEOUT_SECS,
        )
        .await
        .map(|o| !o.status.success())
        .unwrap_or(true);

    if is_new {
        // For new files, return the full content as the diff
        let content_cmd = format!("cat -- {}", shell_quote(&file_path));
        let server = get_server(&server_id)?;
        let ssh_args = build_remote_ssh_args(
            &server,
            &format!("cd {} && {}", shell_quote(&remote_path), content_cmd),
        );
        let mut cmd = tokio::process::Command::new("ssh");
        cmd.args(&ssh_args);
        let label = format!("ssh git cat {}", server.name);
        let output =
            crate::external_command::run_with_timeout(cmd, &label, SSH_GIT_TIMEOUT_SECS).await?;
        let content = String::from_utf8_lossy(&output.stdout).to_string();
        let additions = content.lines().count() as u32;
        return Ok(FileDiff {
            file_path,
            is_new_file: true,
            is_deleted: false,
            is_binary: content.contains('\0'),
            content,
            additions,
            deletions: 0,
        });
    }

    // For tracked files, get the diff
    let git_args = build_git_diff_args(&file_path);
    let output = run_remote_git(&server_id, &remote_path, &git_args, SSH_GIT_TIMEOUT_SECS).await?;

    let content = String::from_utf8_lossy(&output.stdout).to_string();
    let is_binary = content.contains("Binary files differ");

    let mut additions = 0u32;
    let mut deletions = 0u32;
    for line in content.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            additions += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            deletions += 1;
        }
    }

    Ok(FileDiff {
        file_path,
        is_new_file: false,
        is_deleted: false,
        is_binary,
        content,
        additions,
        deletions,
    })
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
    fn remote_git_command_quotes_path_and_keeps_git_command_structure() {
        let command = build_remote_git_command(
            "/tmp/project; touch /tmp/injected",
            "status --porcelain -uno",
        );
        assert_eq!(
            command,
            "cd '/tmp/project; touch /tmp/injected' && git status --porcelain -uno"
        );
    }

    #[test]
    fn git_user_arguments_are_quoted_and_option_safe() {
        assert_eq!(
            build_git_push_args("feature/one; touch /tmp/injected"),
            "push -u origin -- 'feature/one; touch /tmp/injected'"
        );
        assert_eq!(
            build_git_diff_args("src/$(touch /tmp/injected).ts"),
            "diff -- 'src/$(touch /tmp/injected).ts'"
        );
        assert_eq!(
            build_git_file_probe_args("--help; touch /tmp/injected"),
            "ls-files --error-unmatch -- '--help; touch /tmp/injected'"
        );
    }

    #[test]
    fn commit_message_is_one_git_argument() {
        assert_eq!(
            build_git_commit_args("message'; touch /tmp/injected"),
            "add -A && git commit -m 'message'\\''; touch /tmp/injected'"
        );
    }

    #[test]
    fn untracked_file_command_quotes_remote_and_file_paths() {
        let command = build_untracked_file_command(
            "/tmp/project; touch /tmp/injected",
            "src/`touch /tmp/injected`.ts",
        );
        assert_eq!(
            command,
            "cd '/tmp/project; touch /tmp/injected' && cat -- 'src/`touch /tmp/injected`.ts'"
        );
    }
}
