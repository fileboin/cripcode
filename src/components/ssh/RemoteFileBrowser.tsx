/**
 * RemoteFileBrowser — a simple file manager for browsing files on a remote
 * VPS over SSH. Lists directories, opens files for reading, and supports
 * creating directories and deleting files.
 *
 * Reuses `buildFileTree` types from `code.ts` and the remote file wrappers
 * from `remoteFiles.ts`. The existing editor types are reused — the plan
 * says "Editor → File Provider ├── Local └── Remote".
 *
 * @module components/ssh/RemoteFileBrowser
 */

import { useState, useCallback, useEffect } from 'react';
import { Button } from '../primitives/Button';
import { Spinner } from '../primitives/Spinner';
import { useOptionalToast } from '../../contexts/ToastContext';
import { asCommandError, formatCommandError } from '../../lib/errors';
import type { FileEntry, FileContent } from '../../lib/code';
import {
  listRemoteFiles,
  readRemoteFile,
  createRemoteDirectory,
  deleteRemoteFile,
} from '../../lib/remoteFiles';
import type { SshServer } from '../../lib/ssh';
import { RemoteGitPanel } from './RemoteGitPanel';

interface RemoteFileBrowserProps {
  server: SshServer;
  onBack: () => void;
}

export function RemoteFileBrowser({ server, onBack }: RemoteFileBrowserProps) {
  const { showToast } = useOptionalToast();
  const [currentPath, setCurrentPath] = useState('/home');
  const [pathInput, setPathInput] = useState('/home');
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [selectedFile, setSelectedFile] = useState<FileContent | null>(null);
  const [selectedFileName, setSelectedFileName] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [isLoadingFile, setIsLoadingFile] = useState(false);
  const [activeTab, setActiveTab] = useState<'files' | 'git'>('files');

  const loadFiles = useCallback(
    async (path: string) => {
      setIsLoading(true);
      setSelectedFile(null);
      setSelectedFileName(null);
      try {
        const list = await listRemoteFiles(server.id, path);
        setEntries(list);
        setCurrentPath(path);
      } catch (err) {
        showToast(formatCommandError(asCommandError(err)), 'error');
      } finally {
        setIsLoading(false);
      }
    },
    [server.id, showToast]
  );

  useEffect(() => {
    void loadFiles(currentPath);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [server.id]);

  const handleBrowse = () => {
    const trimmed = pathInput.trim();
    if (trimmed) {
      void loadFiles(trimmed);
    }
  };

  const handleEntryClick = async (entry: FileEntry) => {
    if (entry.isDirectory) {
      const newPath = currentPath.endsWith('/')
        ? `${currentPath}${entry.name}`
        : `${currentPath}/${entry.name}`;
      await loadFiles(newPath);
      setPathInput(newPath);
    } else {
      setIsLoadingFile(true);
      setSelectedFile(null);
      const filePath = currentPath.endsWith('/')
        ? `${currentPath}${entry.name}`
        : `${currentPath}/${entry.name}`;
      try {
        const content = await readRemoteFile(server.id, filePath);
        setSelectedFile(content);
        setSelectedFileName(entry.name);
      } catch (err) {
        showToast(formatCommandError(asCommandError(err)), 'error');
      } finally {
        setIsLoadingFile(false);
      }
    }
  };

  const handleGoUp = () => {
    const parts = currentPath.split('/').filter(Boolean);
    if (parts.length > 1) {
      parts.pop();
      const parent = '/' + parts.join('/');
      void loadFiles(parent);
      setPathInput(parent);
    }
  };

  const handleRefresh = () => {
    void loadFiles(currentPath);
  };

  const handleMkdir = async () => {
    const name = window.prompt('Directory name:');
    if (!name) return;
    const dirPath = currentPath.endsWith('/') ? `${currentPath}${name}` : `${currentPath}/${name}`;
    try {
      await createRemoteDirectory(server.id, dirPath);
      showToast('Directory created', 'success');
      void loadFiles(currentPath);
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    }
  };

  const handleDelete = async (entry: FileEntry) => {
    if (!window.confirm(`Delete ${entry.name}?`)) return;
    const path = currentPath.endsWith('/')
      ? `${currentPath}${entry.name}`
      : `${currentPath}/${entry.name}`;
    try {
      await deleteRemoteFile(server.id, path);
      showToast('Deleted', 'info');
      void loadFiles(currentPath);
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    }
  };

  return (
    <div className="ssh-remote-files">
      <div className="ssh-remote-files-header">
        <Button variant="ghost" size="sm" onClick={onBack}>
          ← Back
        </Button>
        <span className="ssh-remote-files-title">
          {server.name} — {currentPath}
        </span>
        <Button
          variant={activeTab === 'files' ? 'secondary' : 'ghost'}
          size="sm"
          onClick={() => setActiveTab('files')}
        >
          Files
        </Button>
        <Button
          variant={activeTab === 'git' ? 'secondary' : 'ghost'}
          size="sm"
          onClick={() => setActiveTab('git')}
        >
          Git
        </Button>
        {activeTab === 'files' && (
          <>
            <Button variant="ghost" size="sm" onClick={handleRefresh}>
              Refresh
            </Button>
            <Button variant="ghost" size="sm" onClick={() => void handleMkdir()}>
              New Folder
            </Button>
          </>
        )}
      </div>

      {activeTab === 'git' ? (
        <RemoteGitPanel server={server} remotePath={currentPath} />
      ) : (
        <>
          <div className="ssh-remote-files-pathbar">
            <input
              type="text"
              value={pathInput}
              onChange={(e) => setPathInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleBrowse();
              }}
              placeholder="/home/user/project"
              className="ssh-remote-files-path-input"
            />
            <Button variant="secondary" size="sm" onClick={handleBrowse}>
              Browse
            </Button>
            <Button variant="ghost" size="sm" onClick={handleGoUp}>
              ↑ Up
            </Button>
          </div>

          <div className="ssh-remote-files-body">
            <div className="ssh-remote-files-list">
              {isLoading ? (
                <div className="ssh-remote-files-loading">
                  <Spinner />
                </div>
              ) : entries.length === 0 ? (
                <p className="ssh-empty-state">No files in this directory.</p>
              ) : (
                <ul className="ssh-file-list">
                  {entries.map((entry) => (
                    <li key={entry.name} className="ssh-file-item">
                      <button
                        className="ssh-file-item-button"
                        onClick={() => void handleEntryClick(entry)}
                      >
                        <span className="ssh-file-icon">{entry.isDirectory ? '📁' : '📄'}</span>
                        <span className="ssh-file-name">{entry.name}</span>
                        {!entry.isDirectory && (
                          <span className="ssh-file-size">{formatSize(entry.size)}</span>
                        )}
                      </button>
                      <button
                        className="ssh-file-delete"
                        onClick={() => void handleDelete(entry)}
                        title="Delete"
                      >
                        ×
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>

            <div className="ssh-remote-files-viewer">
              {isLoadingFile ? (
                <div className="ssh-remote-files-loading">
                  <Spinner />
                </div>
              ) : selectedFile ? (
                <div className="ssh-file-content">
                  <div className="ssh-file-content-header">
                    {selectedFileName}
                    <span className="ssh-file-content-meta">
                      {selectedFile.language} · {formatSize(selectedFile.size)}
                    </span>
                  </div>
                  {selectedFile.isBinary ? (
                    <p className="ssh-empty-state">Binary file — cannot display.</p>
                  ) : selectedFile.isTruncated ? (
                    <p className="ssh-empty-state">
                      File is too large ({formatSize(selectedFile.size)}).
                    </p>
                  ) : (
                    <pre className="ssh-file-content-pre">
                      <code>{selectedFile.content}</code>
                    </pre>
                  )}
                </div>
              ) : (
                <p className="ssh-empty-state">Select a file to view its content.</p>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
