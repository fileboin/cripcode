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
  getOllamaModelInfo,
  getSelectedOllamaModel,
  setSelectedOllamaModel,
  formatModelSize,
  formatContextLength,
  type OllamaStatus,
  type OllamaModel,
  type OllamaModelInfo,
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
  const [selectedModelInfo, setSelectedModelInfo] = useState<OllamaModelInfo | null>(null);
  const [isLoadingModelInfo, setIsLoadingModelInfo] = useState(false);
  const [selectedModel, setSelectedModel] = useState<string | null>(null);

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
        const saved = await getSelectedOllamaModel(selectedServerId);
        setSelectedModel(saved);
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

  const handleModelClick = useCallback(
    async (model: OllamaModel) => {
      setIsLoadingModelInfo(true);
      setSelectedModelInfo(null);
      try {
        const info = await getOllamaModelInfo(selectedServerId, model.name);
        setSelectedModelInfo(info);
      } catch (err) {
        showToast(formatCommandError(asCommandError(err)), 'error');
      } finally {
        setIsLoadingModelInfo(false);
      }
    },
    [selectedServerId, showToast]
  );

  const handleModelSelect = useCallback(
    async (modelName: string) => {
      setSelectedModel(modelName);
      try {
        await setSelectedOllamaModel(selectedServerId, modelName);
        showToast(`Model set to ${modelName}`, 'success');
      } catch (err) {
        showToast(formatCommandError(asCommandError(err)), 'error');
      }
    },
    [selectedServerId, showToast]
  );

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
                      <button
                        className="ssh-file-item-button"
                        style={{ cursor: 'pointer' }}
                        onClick={() => void handleModelClick(model)}
                      >
                        <span className="ssh-file-icon">🤖</span>
                        <span className="ssh-file-name">{model.name}</span>
                        <span className="ssh-file-size">
                          {formatModelSize(model.size)}
                          {model.details ? ` · ${model.details.parameterSize}` : ''}
                          {model.details ? ` · ${model.details.family}` : ''}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}

          {status.running && models.length > 0 && (
            <div className="ssh-ollama-models" style={{ marginTop: 'var(--spacing-sm)' }}>
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 'var(--spacing-sm)',
                }}
              >
                <label
                  style={{
                    fontSize: 'var(--font-size-sm)',
                    color: 'var(--text-secondary)',
                    whiteSpace: 'nowrap',
                  }}
                >
                  AI Model:
                </label>
                <select
                  value={selectedModel ?? ''}
                  onChange={(e) => void handleModelSelect(e.target.value)}
                  className="ssh-remote-files-path-input"
                  style={{ flex: 1, width: 'auto' }}
                >
                  <option value="" disabled>
                    Select a model...
                  </option>
                  {models.map((model) => (
                    <option key={model.name} value={model.name}>
                      {model.name}
                      {model.details ? ` (${model.details.parameterSize})` : ''}
                    </option>
                  ))}
                </select>
              </div>
            </div>
          )}

          {isLoadingModelInfo && (
            <div style={{ padding: 'var(--spacing-sm)', textAlign: 'center' }}>
              <Spinner size="sm" />
            </div>
          )}

          {selectedModelInfo && !isLoadingModelInfo && (
            <div className="ssh-ollama-models" style={{ marginTop: 'var(--spacing-sm)' }}>
              <div className="ssh-server-card" style={{ cursor: 'default' }}>
                <div className="ssh-server-info">
                  <span className="ssh-server-name">{selectedModelInfo.name}</span>
                  <span className="ssh-server-detail">Family: {selectedModelInfo.family}</span>
                  <span className="ssh-server-detail">
                    Parameters: {selectedModelInfo.parameterSize}
                  </span>
                  {selectedModelInfo.quantization && (
                    <span className="ssh-server-detail">
                      Quantization: {selectedModelInfo.quantization}
                    </span>
                  )}
                  <span className="ssh-server-detail">
                    Context: {formatContextLength(selectedModelInfo.contextLength)} tokens
                  </span>
                  {selectedModelInfo.parameterCount !== null && (
                    <span className="ssh-server-detail">
                      Parameter count: {selectedModelInfo.parameterCount.toLocaleString()}
                    </span>
                  )}
                  <span
                    className={`ssh-connection-badge ssh-connection-badge--${
                      selectedModelInfo.loaded ? 'connected' : 'disconnected'
                    }`}
                  >
                    {selectedModelInfo.loaded ? 'Loaded' : 'Not Loaded'}
                  </span>
                </div>
              </div>
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
