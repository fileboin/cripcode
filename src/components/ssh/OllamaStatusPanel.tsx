/**
 * OllamaStatusPanel — shows Ollama connection status and available models.
 *
 * Supports both local and remote (VPS) Ollama detection. The user picks
 * "Local" or a remote SSH server from a dropdown, then clicks "Refresh" to
 * check the status and list models.
 *
 * @module components/ssh/OllamaStatusPanel
 */

import { useState, useCallback } from 'react';
import { Button } from '../primitives/Button';
import { Spinner } from '../primitives/Spinner';
import { useOptionalToast } from '../../contexts/ToastContext';
import { asCommandError, formatCommandError } from '../../lib/errors';
import {
  checkOllamaStatus,
  listOllamaModels,
  formatModelSize,
  type OllamaStatus,
  type OllamaModel,
} from '../../lib/ollama';
import { listSshServers, type SshServer } from '../../lib/ssh';

interface OllamaStatusPanelProps {
  /** Optional pre-selected server for remote mode. */
  defaultServerId?: string | null;
}

export function OllamaStatusPanel({ defaultServerId }: OllamaStatusPanelProps) {
  const { showToast } = useOptionalToast();
  const [servers, setServers] = useState<SshServer[]>([]);
  const [selectedServerId, setSelectedServerId] = useState<string | null>(defaultServerId ?? null);
  const [status, setStatus] = useState<OllamaStatus | null>(null);
  const [models, setModels] = useState<OllamaModel[]>([]);
  const [isChecking, setIsChecking] = useState(false);
  const [isRefreshing, setIsRefreshing] = useState(false);

  // Load SSH servers for the dropdown
  useState(() => {
    void listSshServers().then((list) => {
      setServers(list);
      if (defaultServerId) setSelectedServerId(defaultServerId);
    });
  });

  const handleCheck = useCallback(async () => {
    setIsChecking(true);
    setStatus(null);
    setModels([]);
    try {
      const result = await checkOllamaStatus(selectedServerId);
      setStatus(result);
      if (result.running) {
        const modelList = await listOllamaModels(selectedServerId);
        setModels(modelList);
      }
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    } finally {
      setIsChecking(false);
    }
  }, [selectedServerId, showToast]);

  const handleRefresh = useCallback(async () => {
    setIsRefreshing(true);
    try {
      if (status?.running) {
        const modelList = await listOllamaModels(selectedServerId);
        setModels(modelList);
        showToast(
          `Refreshed — ${modelList.length} model${modelList.length !== 1 ? 's' : ''}`,
          'success'
        );
      } else {
        await handleCheck();
      }
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    } finally {
      setIsRefreshing(false);
    }
  }, [selectedServerId, status, handleCheck, showToast]);

  return (
    <div className="ssh-ollama-panel">
      <div className="ssh-ollama-header">
        <h3 style={{ margin: 0, fontSize: 'var(--font-size-base)' }}>Ollama</h3>
        <div style={{ flex: 1 }} />
        <select
          value={selectedServerId ?? ''}
          onChange={(e) => {
            setSelectedServerId(e.target.value || null);
            setStatus(null);
            setModels([]);
          }}
          className="ssh-remote-files-path-input"
          style={{ width: 'auto', minWidth: '150px' }}
        >
          <option value="">Local</option>
          {servers.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name} (Remote)
            </option>
          ))}
        </select>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => void handleRefresh()}
          disabled={isChecking || isRefreshing}
        >
          {isRefreshing ? 'Refreshing...' : 'Refresh Models'}
        </Button>
      </div>

      {(isChecking || isRefreshing) && !status && (
        <div style={{ padding: 'var(--spacing-md)', textAlign: 'center' }}>
          <Spinner size="sm" />
        </div>
      )}

      {status && (
        <div className="ssh-ollama-status">
          <div className="ssh-server-card" style={{ cursor: 'default' }}>
            <div className="ssh-server-info">
              <span className="ssh-server-name">
                {selectedServerId ? 'Remote Ollama' : 'Local Ollama'}
              </span>
              <span className="ssh-server-detail">{status.endpoint}</span>
              {status.version && (
                <span className="ssh-server-detail">Version: {status.version}</span>
              )}
              <span
                className={`ssh-connection-badge ssh-connection-badge--${
                  status.running ? 'connected' : status.installed ? 'connecting' : 'disconnected'
                }`}
              >
                {status.running ? 'Connected' : status.installed ? 'Not Running' : 'Not Installed'}
              </span>
            </div>
          </div>

          {status.error && (
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

          {status.running && (
            <div className="ssh-ollama-models">
              <h4
                style={{
                  margin: 'var(--spacing-sm) 0 var(--spacing-xs) 0',
                  fontSize: 'var(--font-size-sm)',
                  color: 'var(--text-secondary)',
                }}
              >
                Installed Models ({models.length})
              </h4>
              {models.length === 0 ? (
                <p className="ssh-empty-state">
                  No models installed. Use "ollama pull &lt;model&gt;" to install one.
                </p>
              ) : (
                <ul className="ssh-file-list">
                  {models.map((model) => (
                    <li key={model.name} className="ssh-file-item">
                      <div className="ssh-file-item-button" style={{ cursor: 'default' }}>
                        <span className="ssh-file-icon">🤖</span>
                        <span className="ssh-file-name">{model.name}</span>
                        <span className="ssh-file-size">
                          {formatModelSize(model.size)}
                          {model.details ? ` · ${model.details.parameterSize}` : ''}
                        </span>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </div>
      )}

      {!status && !isChecking && (
        <p
          style={{
            padding: 'var(--spacing-md)',
            fontSize: 'var(--font-size-sm)',
            color: 'var(--text-muted)',
          }}
        >
          Click "Refresh Models" to detect Ollama and list available models.
        </p>
      )}
    </div>
  );
}
