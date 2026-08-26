/**
 * RemoteBuildPanel — run and monitor a build on a remote VPS.
 *
 * Shows: build command input, Start/Stop buttons, status badge (running/
 * success/failed), and a live log viewer. Polls status every 2s while
 * a build is running.
 *
 * @module components/ssh/RemoteBuildPanel
 */

import { useState, useCallback, useEffect, useRef } from 'react';
import { Button } from '../primitives/Button';
import { usePolling } from '../../hooks/usePolling';
import { useOptionalToast } from '../../contexts/ToastContext';
import { asCommandError, formatCommandError } from '../../lib/errors';
import {
  startRemoteBuild,
  stopRemoteBuild,
  getRemoteBuildStatus,
  getRemoteBuildLogs,
  type RemoteBuildStatus,
} from '../../lib/remoteBuild';
import type { SshServer } from '../../lib/ssh';

interface RemoteBuildPanelProps {
  server: SshServer;
  remotePath: string;
}

export function RemoteBuildPanel({ server, remotePath }: RemoteBuildPanelProps) {
  const { showToast } = useOptionalToast();
  const [status, setStatus] = useState<RemoteBuildStatus | null>(null);
  const [logs, setLogs] = useState('');
  const [command, setCommand] = useState('npm run build');
  const [isStarting, setIsStarting] = useState(false);
  const [isStopping, setIsStopping] = useState(false);
  const logRef = useRef<HTMLPreElement>(null);

  const refreshStatus = useCallback(async () => {
    try {
      const s = await getRemoteBuildStatus(server.id, remotePath);
      setStatus(s);
      if (s.running || s.exitCode !== null) {
        const logText = await getRemoteBuildLogs(server.id, remotePath, 200);
        setLogs(logText);
      }
    } catch {
      // Silently degrade
    }
  }, [server.id, remotePath]);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  // Poll status every 2s when running
  usePolling(refreshStatus, {
    intervalMs: 2000,
    enabled: status?.running === true && !isStarting && !isStopping,
  });

  // Auto-scroll logs to bottom
  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [logs]);

  const handleStart = async () => {
    setIsStarting(true);
    try {
      await startRemoteBuild(server.id, remotePath, command.trim() || 'npm run build');
      showToast('Build started', 'success');
      await refreshStatus();
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    } finally {
      setIsStarting(false);
    }
  };

  const handleStop = async () => {
    setIsStopping(true);
    try {
      await stopRemoteBuild(server.id, remotePath);
      showToast('Build stopped', 'info');
      await refreshStatus();
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    } finally {
      setIsStopping(false);
    }
  };

  const isRunning = status?.running === true;
  const isDone = status?.exitCode !== null && !isRunning;
  const isSuccessful = status?.success === true;
  const isFailed = status?.success === false;
  const isBusy = isStarting || isStopping;

  return (
    <div className="ssh-ollama-panel">
      <div className="ssh-ollama-header">
        <h3 style={{ margin: 0, fontSize: 'var(--font-size-base)' }}>Build</h3>
        <div style={{ flex: 1 }} />
        {status && (
          <span
            className={`ssh-connection-badge ssh-connection-badge--${
              isRunning ? 'connecting' : isSuccessful ? 'connected' : isFailed ? 'error' : 'disconnected'
            }`}
          >
            {isRunning ? 'Building...' : isSuccessful ? 'Success' : isFailed ? 'Failed' : 'Idle'}
          </span>
        )}
      </div>

      <div style={{ display: 'flex', gap: 'var(--spacing-sm)', alignItems: 'center' }}>
        <input
          type="text"
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          placeholder="npm run build"
          disabled={isBusy || isRunning}
          className="ssh-remote-files-path-input"
          style={{ flex: 1 }}
        />
      </div>

      <div style={{ display: 'flex', gap: 'var(--spacing-sm)' }}>
        {!isRunning ? (
          <Button
            variant="primary"
            size="sm"
            onClick={() => void handleStart()}
            disabled={isBusy}
          >
            {isStarting ? 'Starting...' : 'Start Build'}
          </Button>
        ) : (
          <Button
            variant="danger"
            size="sm"
            onClick={() => void handleStop()}
            disabled={isStopping}
          >
            {isStopping ? 'Stopping...' : 'Stop'}
          </Button>
        )}
      </div>

      {status?.error && (
        <p
          style={{
            padding: 'var(--spacing-sm) var(--spacing-md)',
            fontSize: 'var(--font-size-sm)',
            color: 'var(--error)',
          }}
        >
          {status.error}
        </p>
      )}

      {(isRunning || isDone) && (
        <div className="ssh-remote-git-diff" style={{ maxHeight: '400px' }}>
          <div className="ssh-file-content-header">
            Build Output
            {isDone && status?.exitCode !== null && (
              <span className="ssh-file-content-meta">
                Exit code: {status?.exitCode}
              </span>
            )}
          </div>
          <pre
            ref={logRef}
            className="ssh-file-content-pre"
            style={{ fontSize: 'var(--font-size-xs)' }}
          >
            <code>{logs || '(no output yet)'}</code>
          </pre>
        </div>
      )}

      {!isRunning && !isDone && !isBusy && (
        <p
          style={{
            padding: 'var(--spacing-md)',
            fontSize: 'var(--font-size-sm)',
            color: 'var(--text-muted)',
          }}
        >
          Enter a build command and click "Start Build".
        </p>
      )}
    </div>
  );
}
