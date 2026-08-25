/**
 * Remote filesystem operations over SSH.
 *
 * Wraps the Rust backend commands for listing, reading, writing, creating,
 * renaming, and deleting files on a remote VPS via SSH. Reuses the same
 * `FileEntry` / `FileContent` types as the local file browser so the
 * frontend can treat local and remote files identically.
 *
 * @module lib/remoteFiles
 */

import { invoke } from '@tauri-apps/api/core';
import type { FileEntry, FileContent } from './code';

/** List files in a remote directory. */
export async function listRemoteFiles(serverId: string, path: string): Promise<FileEntry[]> {
  const entries = await invoke<
    Array<{
      name: string;
      path: string;
      is_directory: boolean;
      size: number;
    }>
  >('list_remote_files', { serverId, path });

  return entries.map((e) => ({
    name: e.name,
    path: e.path,
    isDirectory: e.is_directory,
    size: e.size,
  }));
}

/** Read a remote file's content. */
export async function readRemoteFile(serverId: string, filePath: string): Promise<FileContent> {
  const result = await invoke<{
    content: string;
    is_binary: boolean;
    is_truncated: boolean;
    size: number;
    language: string;
  }>('read_remote_file', { serverId, filePath });

  return {
    content: result.content,
    isBinary: result.is_binary,
    isTruncated: result.is_truncated,
    size: result.size,
    language: result.language,
  };
}

/** Write content to a remote file. */
export async function saveRemoteFile(
  serverId: string,
  filePath: string,
  content: string
): Promise<void> {
  await invoke('save_remote_file', { serverId, filePath, content });
}

/** Create a remote directory (mkdir -p). */
export async function createRemoteDirectory(serverId: string, dirPath: string): Promise<void> {
  await invoke('create_remote_directory', { serverId, dirPath });
}

/** Delete a remote file or directory. */
export async function deleteRemoteFile(serverId: string, path: string): Promise<void> {
  await invoke('delete_remote_file', { serverId, path });
}

/** Rename/move a remote file or directory. */
export async function renameRemoteFile(
  serverId: string,
  oldPath: string,
  newPath: string
): Promise<void> {
  await invoke('rename_remote_file', { serverId, oldPath, newPath });
}
