/**
 * Remote project management.
 *
 * Remote projects are local metadata entries pointing to project folders on
 * a VPS. They appear on the dashboard alongside local projects. When opened,
 * the file browser and terminal route through SSH.
 *
 * @module lib/remoteProjects
 */

import { invoke } from '@tauri-apps/api/core';

export interface RemoteProject {
  id: string;
  name: string;
  serverId: string;
  remotePath: string;
  createdAt: number;
  lastOpened: number | null;
}

export async function listRemoteProjects(): Promise<RemoteProject[]> {
  return invoke<RemoteProject[]>('list_remote_projects');
}

export async function addRemoteProject(
  name: string,
  serverId: string,
  remotePath: string
): Promise<RemoteProject> {
  return invoke<RemoteProject>('add_remote_project', { name, serverId, remotePath });
}

export async function removeRemoteProject(id: string): Promise<void> {
  await invoke('remove_remote_project', { id });
}

export async function markRemoteProjectOpened(id: string): Promise<void> {
  await invoke('mark_remote_project_opened', { id });
}
