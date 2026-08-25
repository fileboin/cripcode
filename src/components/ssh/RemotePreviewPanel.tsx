/**
 * RemotePreviewPanel — preview a remote dev server via SSH port forwarding.
 *
 * Creates an SSH tunnel from a local port to the remote dev server port,
 * then shows a preview iframe pointing at the local port. Reuses the
 * existing proxy/preview infrastructure — the iframe just loads
 * `http://localhost:<tunneled_port>` which the SSH tunnel forwards to the VPS.
 *
 * @module components/ssh/RemotePreviewPanel
 */

import { useState, useCallback, useRef } from 'react';
import { Button } from '../primitives/Button';
import { usePolling } from '../../hooks/usePolling';
import { useOptionalToast } from '../../contexts/ToastContext';
import { asCommandError, formatCommandError } from '../../lib/errors';
import {
  startRemotePreviewTunnel,
  stopRemotePreviewTunnel,
  getRemotePreviewStatus,
  type RemotePreviewStatus,
} from '../../lib/remotePreview';
import type { SshServer } from '../../lib/ssh';

interface RemotePreviewPanelProps {
  server: SshServer;
  remotePath: string;
}

export function RemotePreviewPanel({ server }: RemotePreviewPanelProps) {
  const { showToast } = useOptionalToast();
  const [remotePort, setRemotePort] = useState('3000');
  const [localPort, setLocalPort] = useState<number | null>(null);
  const [status, setStatus] = useState<RemotePreviewStatus | null>(null);
  const [isStarting, setIsStarting] = useState(false);
  const [isStopping, setIsStopping] = useState(false);
  const [iframeKey, setIframeKey] = useState(0);
  const iframeRef = useRef<HTMLIFrameElement>(null);

  const refreshStatus = useCallback(async () => {
    const port = Number(remotePort.trim()) || 3000;
    try {
      const s = await getRemotePreviewStatus(server.id, port);
      setStatus(s);
    } catch {
      // Silently degrade
    }
  }, [server.id, remotePort]);

  usePolling(refreshStatus, {
    intervalMs: 3000,
    enabled: localPort !== null && !isStarting && !isStopping,
  });

  const handleStart = async () => {
    const rPort = Number(remotePort.trim()) || 3000;
    setIsStarting(true);
    try {
      const lPort = await startRemotePreviewTunnel(server.id, rPort);
      setLocalPort(lPort);
      showToast(`Tunnel started: localhost:${lPort} → remote:${rPort}`, 'success');
      setIframeKey((k) => k + 1);
      await refreshStatus();
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    } finally {
      setIsStarting(false);
    }
  };

  const handleStop = async () => {
    const rPort = Number(remotePort.trim()) || 3000;
    setIsStopping(true);
    try {
      await stopRemotePreviewTunnel(server.id, rPort);
      setLocalPort(null);
      setStatus(null);
      showToast('Tunnel stopped', 'info');
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    } finally {
      setIsStopping(false);
    }
  };

  const handleRefresh = () => {
    setIframeKey((k) => k + 1);
    if (iframeRef.current) {
      iframeRef.current.src = `http://localhost:${localPort}`;
    }
  };

  const isTunnelActive = status?.tunnelActive === true;
  const isServerUp = status?.serverResponding === true;
  const previewUrl = localPort ? `http://localhost:${localPort}` : null;

  return (
    <div className="ssh-ollama-panel">
      <div className="ssh-ollama-header">
        <h3 style={{ margin: 0, fontSize: 'var(--font-size-base)' }}>Preview</h3>
        <div style={{ flex: 1 }} />
        {status && (
          <span
            className={`ssh-connection-badge ssh-connection-badge--${
              isServerUp ? 'connected' : isTunnelActive ? 'connecting' : 'disconnected'
            }`}
          >
            {isServerUp ? 'Live' : isTunnelActive ? 'Tunnel Up, No Server' : 'No Tunnel'}
          </span>
        )}
      </div>

      <div style={{ display: 'flex', gap: 'var(--spacing-sm)', alignItems: 'center' }}>
        <label
          style={{
            fontSize: 'var(--font-size-sm)',
            color: 'var(--text-secondary)',
            whiteSpace: 'nowrap',
          }}
        >
          Remote port:
        </label>
        <input
          type="number"
          value={remotePort}
          onChange={(e) => setRemotePort(e.target.value)}
          placeholder="3000"
          disabled={isStarting || isStopping || isTunnelActive}
          className="ssh-remote-files-path-input"
          style={{ width: '80px' }}
          min={1}
          max={65535}
        />
        <div style={{ flex: 1 }} />
        {!isTunnelActive ? (
          <Button
            variant="primary"
            size="sm"
            onClick={() => void handleStart()}
            disabled={isStarting}
          >
            {isStarting ? 'Starting...' : 'Start Tunnel'}
          </Button>
        ) : (
          <>
            <Button variant="ghost" size="sm" onClick={handleRefresh} disabled={isStopping}>
              Refresh
            </Button>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void handleStop()}
              disabled={isStopping}
            >
              {isStopping ? 'Stopping...' : 'Stop Tunnel'}
            </Button>
          </>
        )}
      </div>

      {status?.error && !isServerUp && (
        <p
          style={{
            padding: 'var(--spacing-sm) var(--spacing-md)',
            fontSize: 'var(--font-size-sm)',
            color: 'var(--text-muted)',
          }}
        >
          {status.error}
        </p>
      )}

      {previewUrl && isServerUp && (
        <div
          style={{
            border: '1px solid var(--border)',
            borderRadius: 'var(--radius-md)',
            overflow: 'hidden',
            height: '400px',
          }}
        >
          <iframe
            key={iframeKey}
            ref={iframeRef}
            src={previewUrl}
            style={{ width: '100%', height: '100%', border: 'none' }}
            title="Remote Preview"
            sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
          />
        </div>
      )}

      {previewUrl && isTunnelActive && !isServerUp && (
        <div
          style={{
            padding: 'var(--spacing-xl)',
            textAlign: 'center',
            color: 'var(--text-muted)',
            fontSize: 'var(--font-size-sm)',
          }}
        >
          Tunnel is active (localhost:{localPort} → remote:{remotePort}) but the dev server is not
          responding yet. Start the dev server in the Dev tab.
        </div>
      )}

      {!isTunnelActive && (
        <p
          style={{
            padding: 'var(--spacing-md)',
            fontSize: 'var(--font-size-sm)',
            color: 'var(--text-muted)',
          }}
        >
          Enter the remote dev server port and click "Start Tunnel" to preview it.
        </p>
      )}
    </div>
  );
}
