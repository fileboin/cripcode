/**
 * Remote dev server management over SSH.
 *
 * Wraps the Rust backend commands for starting, stopping, restarting,
 * checking status, and reading logs of a dev server running on a remote VPS.
 *
 * @module lib/remoteDevServer
 */

import { invoke } from '@tauri-apps/api/core';

export interface RemoteDevServerStatus {
  running: boolean;
  pid: number | null;
  port: number | null;
  logLines: number;
  error: string | null;
}

/** Start a dev server on the remote VPS. */
export async function startRemoteDevServer(
  serverId: string,
  remotePath: string,
  command: string,
  port: number | null
): Promise<void> {
  await invoke('start_remote_dev_server', { serverId, remotePath, command, port });
}

/** Stop the dev server on the remote VPS. */
export async function stopRemoteDevServer(serverId: string, remotePath: string): Promise<void> {
  await invoke('stop_remote_dev_server', { serverId, remotePath });
}

/** Restart the dev server (stop + start with the same command). */
export async function restartRemoteDevServer(
  serverId: string,
  remotePath: string,
  command: string,
  port: number | null
): Promise<void> {
  await invoke('restart_remote_dev_server', { serverId, remotePath, command, port });
}

/** Check the status of the dev server on the remote VPS. */
export async function getRemoteDevServerStatus(
  serverId: string,
  remotePath: string
): Promise<RemoteDevServerStatus> {
  return invoke<RemoteDevServerStatus>('get_remote_dev_server_status', {
    serverId,
    remotePath,
  });
}

/** Get recent dev server logs from the VPS. */
export async function getRemoteDevServerLogs(
  serverId: string,
  remotePath: string,
  lines?: number
): Promise<string> {
  return invoke<string>('get_remote_dev_server_logs', {
    serverId,
    remotePath,
    lines: lines ?? null,
  });
}
