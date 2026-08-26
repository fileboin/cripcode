/**
 * RemoteAgentPanel — pick an AI agent and run it on the remote VPS project.
 *
 * Shows a dropdown of available agents (Claude Code, Codex, OpenCode) and
 * a terminal area where the agent runs on the VPS via SSH. The agent CLI
 * is spawned on the VPS, so it works on the remote project's files.
 *
 * @module components/ssh/RemoteAgentPanel
 */

import { useState, useCallback } from 'react';
import { Button } from '../primitives/Button';
import { useOptionalToast } from '../../contexts/ToastContext';
import { asCommandError, formatCommandError } from '../../lib/errors';
import { checkRemoteAgentInstalled } from '../../lib/remoteAgent';
import type { SshServer } from '../../lib/ssh';
import { ALL_AGENTS, type AgentConfig } from '../../lib/agent';
import { RemoteAgentTerminal } from './RemoteAgentTerminal';

interface RemoteAgentPanelProps {
  server: SshServer;
  remotePath: string;
}

export function RemoteAgentPanel({ server, remotePath }: RemoteAgentPanelProps) {
  const { showToast } = useOptionalToast();
  const [selectedAgent, setSelectedAgent] = useState<AgentConfig>(ALL_AGENTS[0]);
  const [agentStatus, setAgentStatus] = useState<
    Record<string, { installed: boolean; path: string | null }>
  >({});
  const [isChecking, setIsChecking] = useState(false);
  const [showTerminal, setShowTerminal] = useState(false);

  const checkAgent = useCallback(
    async (agent: AgentConfig) => {
      setIsChecking(true);
      try {
        const status = await checkRemoteAgentInstalled(server.id, agent.binaryName);
        setAgentStatus((prev) => ({
          ...prev,
          [agent.id]: { installed: status.installed, path: status.path },
        }));
        if (!status.installed) {
          showToast(
            `${agent.displayName} is not installed on ${server.name}. Install it first.`,
            'error'
          );
        }
        return status.installed;
      } catch (err) {
        showToast(formatCommandError(asCommandError(err)), 'error');
        return false;
      } finally {
        setIsChecking(false);
      }
    },
    [server.id, server.name, showToast]
  );

  const handleStart = async () => {
    const installed = await checkAgent(selectedAgent);
    if (installed) {
      setShowTerminal(true);
    }
  };

  if (showTerminal) {
    return (
      <RemoteAgentTerminal
        agent={selectedAgent}
        server={server}
        remotePath={remotePath}
        onBack={() => setShowTerminal(false)}
      />
    );
  }

  return (
    <div className="ssh-ollama-panel">
      <div className="ssh-ollama-header">
        <h3 style={{ margin: 0, fontSize: 'var(--font-size-base)' }}>Remote Agent</h3>
        <div style={{ flex: 1 }} />
      </div>

      <div style={{ display: 'flex', gap: 'var(--spacing-sm)', alignItems: 'center' }}>
        <label
          style={{
            fontSize: 'var(--font-size-sm)',
            color: 'var(--text-secondary)',
            whiteSpace: 'nowrap',
          }}
        >
          Agent:
        </label>
        <select
          value={selectedAgent.id}
          onChange={(e) => {
            const agent = ALL_AGENTS.find((a) => a.id === e.target.value);
            if (agent) setSelectedAgent(agent);
          }}
          className="ssh-remote-files-path-input"
          style={{ flex: 1, width: 'auto' }}
        >
          {ALL_AGENTS.map((agent) => (
            <option key={agent.id} value={agent.id}>
              {agent.displayName}
            </option>
          ))}
        </select>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => void checkAgent(selectedAgent)}
          disabled={isChecking}
        >
          {isChecking ? 'Checking...' : 'Check'}
        </Button>
      </div>

      {agentStatus[selectedAgent.id] && (
        <div className="ssh-server-card" style={{ cursor: 'default' }}>
          <div className="ssh-server-info">
            <span className="ssh-server-name">{selectedAgent.displayName}</span>
            {agentStatus[selectedAgent.id].path && (
              <span className="ssh-server-detail">
                {agentStatus[selectedAgent.id].path}
              </span>
            )}
            <span
              className={`ssh-connection-badge ssh-connection-badge--${
                agentStatus[selectedAgent.id].installed ? 'connected' : 'disconnected'
              }`}
            >
              {agentStatus[selectedAgent.id].installed ? 'Installed' : 'Not Installed'}
            </span>
          </div>
        </div>
      )}

      {agentStatus[selectedAgent.id]?.installed && (
        <Button
          variant="primary"
          size="sm"
          onClick={() => void handleStart()}
          disabled={isChecking}
        >
          Start Agent
        </Button>
      )}

      {!agentStatus[selectedAgent.id] && (
        <p
          style={{
            padding: 'var(--spacing-md)',
            fontSize: 'var(--font-size-sm)',
            color: 'var(--text-muted)',
          }}
        >
          Pick an agent and click "Check" to verify it's installed on the VPS,
          then "Start Agent" to run it on this project.
        </p>
      )}
    </div>
  );
}
