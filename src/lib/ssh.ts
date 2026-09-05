/**
 * SSH server management.
 *
 * Thin wrappers around the Rust backend commands for SSH server CRUD and
 * connection state management.
 *
 * @module lib/ssh
 */

import { invoke } from '@tauri-apps/api/core';

/** How a server authenticates. `key` shells out with the stored key path;
 * `password` reads the password from the OS keystore via the askpass
 * helper. */
export type SshAuthType = 'key' | 'password';

export interface SshServer {
  id: string;
  name: string;
  host: string;
  port: number | null;
  username: string;
  keyPath: string | null;
  authType: SshAuthType;
  createdAt: number;
  lastConnectedAt: number | null;
}

export type SshConnectionState = 'disconnected' | 'connecting' | 'connected' | 'error';

export interface NewSshServer {
  name: string;
  host: string;
  port: number | null;
  username: string;
  keyPath: string | null;
  authType: SshAuthType;
  /**
   * Transient: sent once over IPC and stored ONLY in the OS keystore by the
   * backend. `null` on update means "keep the stored password". It must
   * never be persisted anywhere by the frontend.
   */
  password?: string | null;
}

export async function listSshServers(): Promise<SshServer[]> {
  return invoke<SshServer[]>('list_ssh_servers');
}

export async function addSshServer(config: NewSshServer): Promise<SshServer> {
  return invoke<SshServer>('add_ssh_server', { ...config });
}

export async function updateSshServer(id: string, config: NewSshServer): Promise<SshServer> {
  return invoke<SshServer>('update_ssh_server', { id, ...config });
}

export async function deleteSshServer(id: string): Promise<void> {
  return invoke<void>('delete_ssh_server', { id });
}

export async function testSshConnection(id: string): Promise<string> {
  return invoke<string>('test_ssh_connection', { id });
}

export async function connectSsh(id: string): Promise<void> {
  return invoke<void>('connect_ssh', { id });
}

export async function disconnectSsh(id: string): Promise<void> {
  return invoke<void>('disconnect_ssh', { id });
}

export async function getSshConnectionState(id: string): Promise<SshConnectionState> {
  return invoke<SshConnectionState>('get_ssh_connection_state', { id });
}

export type RemoteHostKeyState = 'known' | 'unknown' | 'changed' | 'probe-unavailable';

export interface HostKeyStatus {
  state: RemoteHostKeyState;
  fingerprint: string | null;
  keyType: string | null;
}

/** Probe the host's key and compare it against the user's known_hosts. */
export async function checkRemoteHostKey(serverId: string): Promise<HostKeyStatus> {
  return invoke<HostKeyStatus>('check_remote_host_key', { serverId });
}

/** Record the user's explicit trust decision in the user's known_hosts. */
export async function acceptRemoteHostKey(serverId: string): Promise<void> {
  return invoke<void>('accept_remote_host_key', { serverId });
}

/** What the UI should do for a probed host key. */
export function resolveHostKeyAction(state: RemoteHostKeyState): 'proceed' | 'prompt' | 'block' {
  switch (state) {
    case 'known':
      return 'proceed';
    case 'changed':
    case 'probe-unavailable':
      // Fail closed: a changed key is an active warning, and an unverifiable
      // host must never fall back to silent TOFU (accept-new).
      return 'block';
    default:
      return 'prompt';
  }
}

const REMOTE_AGENT_BINARIES = new Set(['claude', 'codex', 'opencode', 'cursor-agent']);

/** Quote one value for the POSIX shell used by the remote SSH host. */
export function quoteRemoteShellValue(value: string): string {
  return `'${value.replace(/'/g, "'\\''")}'`;
}

/** Build the remote agent shell program without changing its command semantics. */
export function buildRemoteAgentCommand(remotePath: string, binaryName: string): string {
  if (!REMOTE_AGENT_BINARIES.has(binaryName)) {
    throw new Error(`Unsupported remote agent binary: ${binaryName}`);
  }
  return `cd ${quoteRemoteShellValue(remotePath)} && ${quoteRemoteShellValue(binaryName)}`;
}

/**
 * Build the SSH CLI argument list for an interactive terminal session.
 * Uses keepalive (ServerAliveInterval) so a dropped connection is detected,
 * and `StrictHostKeyChecking=accept-new` for first-connection auto-accept.
 * Unlike the connection test, BatchMode is NOT set — interactive sessions
 * may need to prompt for a key passphrase.
 */
export function buildSshTerminalArgs(server: SshServer): string[] {
  const args = [
    '-o',
    'ConnectTimeout=10',
    '-o',
    'ServerAliveInterval=30',
    '-o',
    'ServerAliveCountMax=3',
    '-o',
    'StrictHostKeyChecking=accept-new',
    '-t',
  ];

  if (server.port && server.port !== 22) {
    args.push('-p', String(server.port));
  }

  if (server.keyPath) {
    args.push('-i', server.keyPath);
  }

  args.push(`${server.username}@${server.host}`);
  return args;
}
