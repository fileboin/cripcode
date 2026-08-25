/**
 * SSH server management.
 *
 * Thin wrappers around the Rust backend commands for SSH server CRUD and
 * connection state management.
 *
 * @module lib/ssh
 */

import { invoke } from '@tauri-apps/api/core';

export interface SshServer {
  id: string;
  name: string;
  host: string;
  port: number | null;
  username: string;
  keyPath: string | null;
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
