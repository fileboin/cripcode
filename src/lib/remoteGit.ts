/**
 * Remote Git operations over SSH.
 *
 * Mirrors the essential local git functions but executes them on a remote VPS
 * via the backend's SSH exec commands. Reuses the same `ChangedFile` and
 * `FileDiff` types as the local git wrapper so the frontend can treat local
 * and remote git identically.
 *
 * @module lib/remoteGit
 */

import { invoke } from '@tauri-apps/api/core';
import type { ChangedFile, FileDiff } from './git';

export interface RemoteBranchInfo {
  name: string;
  isCurrent: boolean;
  isRemote: boolean;
  lastCommitDate: number;
  lastCommitAuthor: string;
}

/** Check if the remote project has uncommitted changes. */
export async function remoteGitStatus(serverId: string, remotePath: string): Promise<boolean> {
  return invoke<boolean>('remote_git_status', { serverId, remotePath });
}

/** Get the current branch name on the remote VPS. */
export async function remoteGitCurrentBranch(
  serverId: string,
  remotePath: string
): Promise<string | null> {
  return invoke<string | null>('remote_git_current_branch', { serverId, remotePath });
}

/** List branches on the remote VPS. */
export async function remoteGitListBranches(
  serverId: string,
  remotePath: string
): Promise<RemoteBranchInfo[]> {
  return invoke<RemoteBranchInfo[]>('remote_git_list_branches', { serverId, remotePath });
}

/** Get changed files on the remote VPS. */
export async function remoteGitChangedFiles(
  serverId: string,
  remotePath: string
): Promise<ChangedFile[]> {
  return invoke<ChangedFile[]>('remote_git_changed_files', { serverId, remotePath });
}

/** Stage all changes and commit on the remote VPS. */
export async function remoteGitCommit(
  serverId: string,
  remotePath: string,
  message: string
): Promise<boolean> {
  return invoke<boolean>('remote_git_commit', { serverId, remotePath, message });
}

/** Pull latest changes from remote on the VPS. */
export async function remoteGitPull(serverId: string, remotePath: string): Promise<void> {
  await invoke('remote_git_pull', { serverId, remotePath });
}

/** Push the current branch to origin on the VPS. */
export async function remoteGitPush(
  serverId: string,
  remotePath: string,
  branchName: string
): Promise<void> {
  await invoke('remote_git_push', { serverId, remotePath, branchName });
}

/** Get the diff for a single file on the remote VPS. */
export async function remoteGitDiff(
  serverId: string,
  remotePath: string,
  filePath: string
): Promise<FileDiff> {
  const result = await invoke<{
    file_path: string;
    is_new_file: boolean;
    is_deleted: boolean;
    is_binary: boolean;
    content: string;
    additions: number;
    deletions: number;
  }>('remote_git_diff', { serverId, remotePath, filePath });

  return {
    filePath: result.file_path,
    isNewFile: result.is_new_file,
    isDeleted: result.is_deleted,
    isBinary: result.is_binary,
    content: result.content,
    additions: result.additions,
    deletions: result.deletions,
  };
}
