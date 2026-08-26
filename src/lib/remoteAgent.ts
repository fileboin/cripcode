/**
 * Remote agent detection over SSH.
 *
 * Checks whether an AI agent CLI (Claude Code, Codex, OpenCode) is installed
 * on a remote VPS. The agent runs ON the VPS — the local machine only needs
 * SSH access, not the agent CLI.
 *
 * @module lib/remoteAgent
 */

import { invoke } from '@tauri-apps/api/core';

export interface RemoteAgentStatus {
  installed: boolean;
  path: string | null;
  binaryName: string;
  error: string | null;
}

/** Check if an agent CLI is installed on a remote VPS. */
export async function checkRemoteAgentInstalled(
  serverId: string,
  binaryName: string
): Promise<RemoteAgentStatus> {
  return invoke<RemoteAgentStatus>('check_remote_agent_installed', {
    serverId,
    binaryName,
  });
}
