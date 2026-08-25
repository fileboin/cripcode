/**
 * RemoteDevServerPanel — manage a dev server running on a remote VPS.
 *
 * Shows: start/stop/restart buttons, status badge, port input, dev command
 * input, and a live log viewer. The panel polls status every 3s when the
 * server is running.
 *
 * @module components/ssh/RemoteDevServerPanel
 */

import { useState, useCallback, useEffect, useRef } from 'react';
import { Button } from '../primitives/Button';
import { usePolling } from '../../hooks/usePolling';
import { useOptionalToast } from '../../contexts/ToastContext';
import { asCommandError, formatCommandError } from '../../lib/errors';
import {
  startRemoteDevServer,
  stopRemoteDevServer,
  restartRemoteDevServer,
  getRemoteDevServerStatus,
  getRemoteDevServerLogs,
  type RemoteDevServerStatus,
} from '../../lib/remoteDevServer';
import type { SshServer } from '../../lib/ssh';

interface RemoteDevServerPanelProps {
  server: SshServer;
  remotePath: string;
}

export function RemoteDevServerPanel({ server, remotePath }: RemoteDevServerPanelProps) {
  const { showToast } = useOptionalToast();
  const [status, setStatus] = useState<RemoteDevServerStatus | null>(null);
  const [logs, setLogs] = useState('');
  const [command, setCommand] = useState('npm run dev');
  const [port, setPort] = useState('3000');
  const [isStarting, setIsStarting] = useState(false);
  const [isStopping, setIsStopping] = useState(false);
  const [isRestarting, setIsRestarting] = useState(false);
  const logRef = useRef<HTMLPreElement>(null);

  const refreshStatus = useCallback(async () => {
    try {
      const s = await getRemoteDevServerStatus(server.id, remotePath);
      setStatus(s);
      if (s.running) {
        const logText = await getRemoteDevServerLogs(server.id, remotePath, 100);
        setLogs(logText);
      }
    } catch {
      // Silently degrade
    }
  }, [server.id, remotePath]);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  // Poll status every 3s when running
  usePolling(refreshStatus, {
    intervalMs: 3000,
    enabled: status?.running === true && !isStarting && !isStopping && !isRestarting,
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
      await startRemoteDevServer(
        server.id,
        remotePath,
        command.trim() || 'npm run dev',
        port.trim() ? Number(port.trim()) : null
      );
      showToast('Dev server started', 'success');
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
      await stopRemoteDevServer(server.id, remotePath);
      showToast('Dev server stopped', 'info');
      await refreshStatus();
      setLogs('');
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    } finally {
      setIsStopping(false);
    }
  };

  const handleRestart = async () => {
    setIsRestarting(true);
    try {
      await restartRemoteDevServer(
        server.id,
        remotePath,
        command.trim() || 'npm run dev',
        port.trim() ? Number(port.trim()) : null
      );
      showToast('Dev server restarted', 'success');
      await refreshStatus();
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    } finally {
      setIsRestarting(false);
    }
  };

  const isRunning = status?.running === true;
  const isBusy = isStarting || isStopping || isRestarting;

  return (
    <div className="ssh-ollama-panel">
      <div className="ssh-ollama-header">
        <h3 style={{ margin: 0, fontSize: 'var(--font-size-base)' }}>Dev Server</h3>
        <div style={{ flex: 1 }} />
        {status && (
          <span
            className={`ssh-connection-badge ssh-connection-badge--${
              isRunning ? 'connected' : 'disconnected'
            }`}
          >
            {isRunning ? 'Running' : 'Stopped'}
          </span>
        )}
      </div>

      <div style={{ display: 'flex', gap: 'var(--spacing-sm)', alignItems: 'center' }}>
        <input
          type="text"
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          placeholder="npm run dev"
          disabled={isBusy || isRunning}
          className="ssh-remote-files-path-input"
          style={{ flex: 1 }}
        />
        <input
          type="number"
          value={port}
          onChange={(e) => setPort(e.target.value)}
          placeholder="3000"
          disabled={isBusy || isRunning}
          className="ssh-remote-files-path-input"
          style={{ width: '80px' }}
          min={1}
          max={65535}
        />
      </div>

      <div style={{ display: 'flex', gap: 'var(--spacing-sm)' }}>
        {!isRunning ? (
          <Button variant="primary" size="sm" onClick={() => void handleStart()} disabled={isBusy}>
            {isStarting ? 'Starting...' : 'Start'}
          </Button>
        ) : (
          <>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void handleStop()}
              disabled={isBusy}
            >
              {isStopping ? 'Stopping...' : 'Stop'}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void handleRestart()}
              disabled={isBusy}
            >
              {isRestarting ? 'Restarting...' : 'Restart'}
            </Button>
          </>
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

      {isRunning && (
        <div className="ssh-remote-git-diff" style={{ maxHeight: '300px' }}>
          <div className="ssh-file-content-header">
            Logs
            {status.pid && <span className="ssh-file-content-meta">PID: {status.pid}</span>}
          </div>
          <pre
            ref={logRef}
            className="ssh-file-content-pre"
            style={{ fontSize: 'var(--font-size-xs)' }}
          >
            <code>{logs || '(no logs yet)'}</code>
          </pre>
        </div>
      )}

      {!isRunning && !isBusy && (
        <p
          style={{
            padding: 'var(--spacing-md)',
            fontSize: 'var(--font-size-sm)',
            color: 'var(--text-muted)',
          }}
        >
          Enter a dev command and port, then click Start.
        </p>
      )}
    </div>
  );
}
