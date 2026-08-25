/**
 * Remote preview via SSH port forwarding.
 *
 * Wraps the Rust backend commands for starting/stopping SSH tunnels
 * and probing the remote dev server's HTTP status through the tunnel.
 *
 * @module lib/remotePreview
 */

import { invoke } from '@tauri-apps/api/core';

export interface RemotePreviewStatus {
  tunnelActive: boolean;
  localPort: number | null;
  remotePort: number | null;
  serverResponding: boolean;
  httpStatus: number | null;
  error: string | null;
}

/** Start an SSH port-forwarding tunnel. Returns the local port. */
export async function startRemotePreviewTunnel(
  serverId: string,
  remotePort: number,
  localPort?: number
): Promise<number> {
  return invoke<number>('start_remote_preview_tunnel', {
    serverId,
    remotePort,
    localPort: localPort ?? null,
  });
}

/** Stop the SSH tunnel. */
export async function stopRemotePreviewTunnel(serverId: string, remotePort: number): Promise<void> {
  await invoke('stop_remote_preview_tunnel', { serverId, remotePort });
}

/** Check the status of the remote preview (tunnel + dev server). */
export async function getRemotePreviewStatus(
  serverId: string,
  remotePort: number
): Promise<RemotePreviewStatus> {
  return invoke<RemotePreviewStatus>('get_remote_preview_status', {
    serverId,
    remotePort,
  });
}
