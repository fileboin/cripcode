/**
 * RemoteGitPanel — Git status and operations for a remote project on a VPS.
 *
 * Shows: current branch, changed files list, commit/pull/push buttons.
 * Reuses existing `ChangedFile` / `FileDiff` types so the UI matches the
 * local git experience. The panel is shown when a remote project is opened.
 *
 * @module components/ssh/RemoteGitPanel
 */

import { useState, useCallback, useEffect } from 'react';
import { Button } from '../primitives/Button';
import { Spinner } from '../primitives/Spinner';
import { usePolling } from '../../hooks/usePolling';
import { useOptionalToast } from '../../contexts/ToastContext';
import { asCommandError, formatCommandError } from '../../lib/errors';
import type { ChangedFile, FileDiff } from '../../lib/git';
import {
  remoteGitStatus,
  remoteGitCurrentBranch,
  remoteGitChangedFiles,
  remoteGitCommit,
  remoteGitPull,
  remoteGitPush,
  remoteGitDiff,
} from '../../lib/remoteGit';
import type { SshServer } from '../../lib/ssh';

interface RemoteGitPanelProps {
  server: SshServer;
  remotePath: string;
}

export function RemoteGitPanel({ server, remotePath }: RemoteGitPanelProps) {
  const { showToast } = useOptionalToast();
  const [currentBranch, setCurrentBranch] = useState<string | null>(null);
  const [hasChanges, setHasChanges] = useState(false);
  const [changedFiles, setChangedFiles] = useState<ChangedFile[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [commitMessage, setCommitMessage] = useState('');
  const [isCommitting, setIsCommitting] = useState(false);
  const [isPulling, setIsPulling] = useState(false);
  const [isPushing, setIsPushing] = useState(false);
  const [selectedDiff, setSelectedDiff] = useState<FileDiff | null>(null);
  const [selectedDiffFile, setSelectedDiffFile] = useState<string | null>(null);

  const refreshStatus = useCallback(async () => {
    try {
      const [branch, changed, hasCh] = await Promise.all([
        remoteGitCurrentBranch(server.id, remotePath),
        remoteGitChangedFiles(server.id, remotePath),
        remoteGitStatus(server.id, remotePath),
      ]);
      setCurrentBranch(branch);
      setChangedFiles(changed);
      setHasChanges(hasCh);
    } catch {
      // Silently degrade — git may not be initialized on the remote path
      setCurrentBranch(null);
      setChangedFiles([]);
      setHasChanges(false);
    } finally {
      setIsLoading(false);
    }
  }, [server.id, remotePath]);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  // Poll for changes every 5s
  usePolling(refreshStatus, {
    intervalMs: 5000,
    enabled: !isCommitting && !isPulling && !isPushing,
  });

  const handleCommit = async () => {
    const msg = commitMessage.trim();
    if (!msg) {
      showToast('Commit message is required', 'error');
      return;
    }
    setIsCommitting(true);
    try {
      const committed = await remoteGitCommit(server.id, remotePath, msg);
      if (committed) {
        showToast('Committed successfully', 'success');
        setCommitMessage('');
        await refreshStatus();
      } else {
        showToast('Nothing to commit', 'info');
      }
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    } finally {
      setIsCommitting(false);
    }
  };

  const handlePull = async () => {
    setIsPulling(true);
    try {
      await remoteGitPull(server.id, remotePath);
      showToast('Pull complete', 'success');
      await refreshStatus();
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    } finally {
      setIsPulling(false);
    }
  };

  const handlePush = async () => {
    if (!currentBranch) {
      showToast('No current branch to push', 'error');
      return;
    }
    setIsPushing(true);
    try {
      await remoteGitPush(server.id, remotePath, currentBranch);
      showToast('Push complete', 'success');
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    } finally {
      setIsPushing(false);
    }
  };

  const handleViewDiff = async (file: ChangedFile) => {
    setSelectedDiffFile(file.path);
    setSelectedDiff(null);
    try {
      const diff = await remoteGitDiff(server.id, remotePath, file.path);
      setSelectedDiff(diff);
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    }
  };

  if (isLoading) {
    return (
      <div style={{ padding: 'var(--spacing-lg)', textAlign: 'center' }}>
        <Spinner size="sm" />
      </div>
    );
  }

  return (
    <div className="ssh-remote-git">
      <div className="ssh-remote-git-header">
        <span className="ssh-remote-git-branch">
          {currentBranch ? `\uE0A0 ${currentBranch}` : 'no branch'}
        </span>
        {hasChanges && (
          <span className="ssh-connection-badge ssh-connection-badge--connecting">
            {changedFiles.length} change{changedFiles.length !== 1 ? 's' : ''}
          </span>
        )}
        <div style={{ flex: 1 }} />
        <Button
          variant="ghost"
          size="sm"
          onClick={() => void handlePull()}
          disabled={isPulling || isPushing || isCommitting}
        >
          {isPulling ? 'Pulling...' : 'Pull'}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => void handlePush()}
          disabled={isPulling || isPushing || isCommitting || !currentBranch}
        >
          {isPushing ? 'Pushing...' : 'Push'}
        </Button>
      </div>

      {changedFiles.length > 0 && (
        <div className="ssh-remote-git-changes">
          <ul className="ssh-file-list">
            {changedFiles.map((file) => (
              <li key={file.path} className="ssh-file-item">
                <button className="ssh-file-item-button" onClick={() => void handleViewDiff(file)}>
                  <span className="ssh-file-icon">
                    {file.status === 'added' ? '+' : file.status === 'deleted' ? '-' : 'M'}
                  </span>
                  <span className="ssh-file-name">{file.path}</span>
                  <span className="ssh-file-size">{file.status}</span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {changedFiles.length > 0 && (
        <div className="ssh-remote-git-commit">
          <input
            type="text"
            value={commitMessage}
            onChange={(e) => setCommitMessage(e.target.value)}
            placeholder="Commit message..."
            disabled={isCommitting}
            className="ssh-remote-files-path-input"
            onKeyDown={(e) => {
              if (e.key === 'Enter') void handleCommit();
            }}
          />
          <Button
            variant="primary"
            size="sm"
            onClick={() => void handleCommit()}
            disabled={isCommitting || !commitMessage.trim()}
          >
            {isCommitting ? 'Committing...' : 'Commit'}
          </Button>
        </div>
      )}

      {selectedDiffFile && (
        <div className="ssh-remote-git-diff">
          <div className="ssh-file-content-header">
            {selectedDiffFile}
            <button
              className="ssh-file-delete"
              onClick={() => {
                setSelectedDiff(null);
                setSelectedDiffFile(null);
              }}
            >
              ×
            </button>
          </div>
          {selectedDiff ? (
            selectedDiff.isBinary ? (
              <p className="ssh-empty-state">Binary file — diff not shown.</p>
            ) : (
              <pre className="ssh-file-content-pre">
                <code>{selectedDiff.content}</code>
              </pre>
            )
          ) : (
            <div className="ssh-remote-files-loading">
              <Spinner size="sm" />
            </div>
          )}
        </div>
      )}

      {changedFiles.length === 0 && !selectedDiffFile && (
        <p
          style={{
            padding: 'var(--spacing-md)',
            fontSize: 'var(--font-size-sm)',
            color: 'var(--text-muted)',
          }}
        >
          Working tree clean — no uncommitted changes.
        </p>
      )}
    </div>
  );
}
