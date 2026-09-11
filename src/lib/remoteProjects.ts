/**
 * Remote project management.
 *
 * Remote projects are local metadata entries pointing to project folders on
 * a VPS. They appear on the dashboard alongside local projects. When opened,
 * the file browser and terminal route through SSH.
 *
 * `localProjectPath` optionally links a remote project to a local repo so the
 * execution-target picker can prefill it. Metadata only — no clone/sync.
 *
 * @module lib/remoteProjects
 */

import { invoke } from '@tauri-apps/api/core';

export interface RemoteProject {
  id: string;
  name: string;
  serverId: string;
  remotePath: string;
  localProjectPath: string | null;
  createdAt: number;
  lastOpened: number | null;
}

export async function listRemoteProjects(): Promise<RemoteProject[]> {
  return invoke<RemoteProject[]>('list_remote_projects');
}

export async function addRemoteProject(
  name: string,
  serverId: string,
  remotePath: string,
  localProjectPath: string | null = null
): Promise<RemoteProject> {
  return invoke<RemoteProject>('add_remote_project', {
    name,
    serverId,
    remotePath,
    localProjectPath,
  });
}

export async function removeRemoteProject(id: string): Promise<void> {
  await invoke('remove_remote_project', { id });
}

export async function markRemoteProjectOpened(id: string): Promise<void> {
  await invoke('mark_remote_project_opened', { id });
}

/** Link (or unlink, with `null`) a remote project to a local project path. */
export async function setRemoteProjectLocalLink(
  id: string,
  localProjectPath: string | null
): Promise<RemoteProject> {
  return invoke<RemoteProject>('set_remote_project_local_link', { id, localProjectPath });
}

/** The remote project linked to a local project path, if any. */
export function findRemoteProjectForLocalPath(
  projects: RemoteProject[],
  localProjectPath: string | null | undefined
): RemoteProject | null {
  if (!localProjectPath) return null;
  return projects.find((project) => project.localProjectPath === localProjectPath) ?? null;
}
