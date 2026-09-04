/**
 * ServerList — displays SSH server configurations with connection state
 * and connect/disconnect/test/edit/delete actions.
 *
 * Uses existing primitives: `<Button variant>`, `useAsyncState`,
 * `usePolling`, `useOptionalToast`. The AddServerModal is managed
 * internally.
 *
 * @module components/ssh/ServerList
 */

import { useState, useCallback } from 'react';
import { Button } from '../primitives/Button';
import { Spinner } from '../primitives/Spinner';
import { useAsyncState } from '../../hooks/useAsyncState';
import { usePolling } from '../../hooks/usePolling';
import { useHostKeyGate } from '../../hooks/useHostKeyGate';
import { useOptionalToast } from '../../contexts/ToastContext';
import { asCommandError, formatCommandError } from '../../lib/errors';
import {
  listSshServers,
  deleteSshServer,
  connectSsh,
  disconnectSsh,
  testSshConnection,
  getSshConnectionState,
  type SshServer,
  type SshConnectionState,
} from '../../lib/ssh';
import { AddServerModal } from './AddServerModal';
import { RemoteTerminal } from './RemoteTerminal';
import { RemoteFileBrowser } from './RemoteFileBrowser';
import { OllamaStatusPanel } from './OllamaStatusPanel';

export function ServerList() {
  const { showToast } = useOptionalToast();
  const [servers, setServers] = useState<SshServer[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [connStates, setConnStates] = useState<Record<string, SshConnectionState>>({});
  const [modalOpen, setModalOpen] = useState(false);
  const [editServer, setEditServer] = useState<SshServer | null>(null);
  const [terminalServer, setTerminalServer] = useState<SshServer | null>(null);
  const [filesServer, setFilesServer] = useState<SshServer | null>(null);
  const [ollamaServer, setOllamaServer] = useState<SshServer | null>(null);
  const [showLocalOllama, setShowLocalOllama] = useState(false);
  const { ensureHostKeyAccepted, hostKeyModal } = useHostKeyGate();

  const loadServers = useCallback(async () => {
    try {
      const list = await listSshServers();
      setServers(list);
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    } finally {
      setIsLoading(false);
    }
  }, [showToast]);

  useAsyncState(loadServers, { immediate: true });

  const refreshStates = useCallback(async () => {
    if (servers.length === 0) return;
    const entries = await Promise.all(
      servers.map(async (s) => {
        try {
          const state = await getSshConnectionState(s.id);
          return [s.id, state] as const;
        } catch {
          return [s.id, 'disconnected' as const] as const;
        }
      })
    );
    setConnStates(Object.fromEntries(entries));
  }, [servers]);

  usePolling(refreshStates, { intervalMs: 3000, enabled: servers.length > 0 });

  const handleAdd = () => {
    setEditServer(null);
    setModalOpen(true);
  };

  const handleEdit = (server: SshServer) => {
    setEditServer(server);
    setModalOpen(true);
  };

  const handleDelete = async (server: SshServer) => {
    try {
      await deleteSshServer(server.id);
      await loadServers();
      showToast('Server deleted', 'info');
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    }
  };

  const handleConnect = async (server: SshServer) => {
    if (!(await ensureHostKeyAccepted(server))) return;
    try {
      await connectSsh(server.id);
      await refreshStates();
      showToast(`Connected to ${server.name}`, 'success');
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    }
  };

  const handleDisconnect = async (server: SshServer) => {
    try {
      await disconnectSsh(server.id);
      await refreshStates();
      showToast(`Disconnected from ${server.name}`, 'info');
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    }
  };

  const handleOpenTerminal = async (server: SshServer) => {
    if (!(await ensureHostKeyAccepted(server))) return;
    setTerminalServer(server);
  };

  const handleOpenFiles = async (server: SshServer) => {
    if (!(await ensureHostKeyAccepted(server))) return;
    setFilesServer(server);
  };

  const handleOpenOllama = async (server: SshServer) => {
    if (!(await ensureHostKeyAccepted(server))) return;
    setOllamaServer(server);
  };

  const handleTest = async (server: SshServer) => {
    if (!(await ensureHostKeyAccepted(server))) return;
    try {
      await testSshConnection(server.id);
      await refreshStates();
      showToast(`Connection to ${server.name} successful`, 'success');
    } catch (err) {
      await refreshStates();
      showToast(formatCommandError(asCommandError(err)), 'error');
    }
  };

  const handleSaved = useCallback(() => {
    void loadServers();
  }, [loadServers]);

  if (isLoading) {
    return (
      <div style={{ padding: 'var(--spacing-xl)', textAlign: 'center' }}>
        <Spinner />
      </div>
    );
  }

  if (terminalServer) {
    return <RemoteTerminal server={terminalServer} onBack={() => setTerminalServer(null)} />;
  }

  if (filesServer) {
    return <RemoteFileBrowser server={filesServer} onBack={() => setFilesServer(null)} />;
  }

  if (ollamaServer) {
    return (
      <div style={{ padding: 'var(--spacing-lg)' }}>
        <OllamaStatusPanel defaultServerId={ollamaServer.id} />
        <div style={{ marginTop: 'var(--spacing-lg)' }}>
          <Button variant="ghost" size="sm" onClick={() => setOllamaServer(null)}>
            ← Back
          </Button>
        </div>
      </div>
    );
  }

  if (showLocalOllama) {
    return (
      <div style={{ padding: 'var(--spacing-lg)' }}>
        <OllamaStatusPanel defaultServerId={null} />
        <div style={{ marginTop: 'var(--spacing-lg)' }}>
          <Button variant="ghost" size="sm" onClick={() => setShowLocalOllama(false)}>
            ← Back
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div style={{ padding: 'var(--spacing-lg)' }}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          marginBottom: 'var(--spacing-md)',
        }}
      >
        <h3 style={{ margin: 0, fontSize: 'var(--font-size-lg)' }}>SSH Servers</h3>
        <div style={{ display: 'flex', gap: 'var(--spacing-sm)' }}>
          <Button variant="secondary" size="sm" onClick={() => setShowLocalOllama(true)}>
            Local Ollama
          </Button>
          <Button variant="primary" size="sm" onClick={handleAdd}>
            Add Server
          </Button>
        </div>
      </div>

      {servers.length === 0 ? (
        <p className="ssh-empty-state">
          No SSH servers configured. Click "Add Server" to connect to a VPS.
        </p>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--spacing-sm)' }}>
          {servers.map((server) => {
            const state = connStates[server.id] ?? 'disconnected';
            return (
              <div key={server.id} className="ssh-server-card">
                <div className="ssh-server-info">
                  <span className="ssh-server-name">{server.name}</span>
                  <span className="ssh-server-detail">
                    {server.username}@{server.host}
                    {server.port && server.port !== 22 ? `:${server.port}` : ''}
                  </span>
                  {server.keyPath && <span className="ssh-server-detail">{server.keyPath}</span>}
                  <span className={`ssh-connection-badge ssh-connection-badge--${state}`}>
                    {state}
                  </span>
                </div>
                <div className="ssh-server-actions">
                  <Button variant="ghost" size="sm" onClick={() => void handleOpenTerminal(server)}>
                    Terminal
                  </Button>
                  <Button variant="ghost" size="sm" onClick={() => void handleOpenFiles(server)}>
                    Files
                  </Button>
                  <Button variant="ghost" size="sm" onClick={() => void handleOpenOllama(server)}>
                    Ollama
                  </Button>
                  <Button variant="ghost" size="sm" onClick={() => void handleTest(server)}>
                    Test
                  </Button>
                  {state === 'connected' ? (
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => void handleDisconnect(server)}
                    >
                      Disconnect
                    </Button>
                  ) : (
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => void handleConnect(server)}
                    >
                      Connect
                    </Button>
                  )}
                  <Button variant="ghost" size="sm" onClick={() => handleEdit(server)}>
                    Edit
                  </Button>
                  <Button variant="ghost" size="sm" onClick={() => void handleDelete(server)}>
                    Delete
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <AddServerModal
        isOpen={modalOpen}
        onClose={() => setModalOpen(false)}
        onSaved={handleSaved}
        editServer={editServer}
        ensureHostKeyAccepted={ensureHostKeyAccepted}
      />

      {hostKeyModal}
    </div>
  );
}
