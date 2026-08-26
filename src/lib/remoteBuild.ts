/**
 * Remote build management over SSH.
 *
 * Wraps the Rust backend commands for starting, stopping, checking status,
 * and reading logs of a build running on a remote VPS.
 *
 * @module lib/remoteBuild
 */

import { invoke } from '@tauri-apps/api/core';

export interface RemoteBuildStatus {
  running: boolean;
  exitCode: number | null;
  success: boolean | null;
  logLines: number;
  error: string | null;
}

/** Start a build on the remote VPS. */
export async function startRemoteBuild(
  serverId: string,
  remotePath: string,
  command: string
): Promise<void> {
  await invoke('start_remote_build', { serverId, remotePath, command });
}

/** Stop a running build on the VPS. */
export async function stopRemoteBuild(
  serverId: string,
  remotePath: string
): Promise<void> {
  await invoke('stop_remote_build', { serverId, remotePath });
}

/** Check the status of a build on the remote VPS. */
export async function getRemoteBuildStatus(
  serverId: string,
  remotePath: string
): Promise<RemoteBuildStatus> {
  return invoke<RemoteBuildStatus>('get_remote_build_status', {
    serverId,
    remotePath,
  });
}

/** Get recent build logs from the VPS. */
export async function getRemoteBuildLogs(
  serverId: string,
  remotePath: string,
  lines?: number
): Promise<string> {
  return invoke<string>('get_remote_build_logs', {
    serverId,
    remotePath,
    lines: lines ?? null,
  });
}
