/**
 * AgentLaunchPanel — one visible place to launch coding agents, LOCAL
 * (built-in CLI on this machine) or REMOTE (agent CLI on a VPS over the
 * existing SSH/PTY flow).
 *
 * - Agents come from `ALL_AGENTS` (generic definition list — adding a new
 *   agent is a data change, no new infrastructure).
 * - A REMOTE binding (SSH server + remote project) is persisted per agent in
 *   `remoteAgentBinding` — configured once, shown as connected on every app
 *   open, Change/Switch to move it, Disconnect to return to LOCAL.
 * - Start (REMOTE) reuses the proven `RemoteAgentTerminal` pty flow; the
 *   binding survives restarts and Start re-attaches the same pty session.
 * - Disconnect kills the remote pty and clears ONLY the binding — the SSH
 *   server, credentials, remote project and configs stay untouched.
 *
 * @module components/dashboard/AgentLaunchPanel
 */

import { useCallback, useEffect, useState } from 'react';
import { Button } from '../primitives/Button';
import { ModalFrame } from '../primitives/ModalFrame';
import { Dropdown, DropdownItem } from '../primitives/Dropdown';
import { ChevronIcon } from '../icons';
import { ALL_AGENTS, type AgentConfig } from '../../lib/agent';
import {
  clearRemoteAgentBinding,
  getRemoteAgentBinding,
  setRemoteAgentBinding,
  subscribeRemoteAgentBindings,
  type RemoteAgentBinding,
} from '../../lib/remoteAgentBinding';
import { checkRemoteAgentInstalled } from '../../lib/remoteAgent';
import { getRemoteAgentSessions } from '../../lib/remoteAgentSessions';
import { listRemoteProjects, type RemoteProject } from '../../lib/remoteProjects';
import { listSshServers, type SshServer } from '../../lib/ssh';
import { asCommandError, formatCommandError } from '../../lib/errors';
import { useOptionalToast } from '../../contexts/ToastContext';
import { useHostKeyGate } from '../../hooks/useHostKeyGate';
import { RemoteAgentTerminal } from '../ssh/RemoteAgentTerminal';
import { killPtySession } from '../../lib/ptySession';

function ptySessionIdFor(serverId: string, agentId: string, path: string): string {
  return `ssh-agent-${serverId}-${agentId}-${path.replace(/[^a-zA-Z0-9]/g, '-')}`;
}

interface PickerState {
  agent: AgentConfig;
  serverId: string;
  projectId: string;
}

interface StartingState {
  agent: AgentConfig;
  server: SshServer;
  project: RemoteProject;
}

export function AgentLaunchPanel() {
  const { showToast } = useOptionalToast();
  const { ensureHostKeyAccepted, hostKeyModal } = useHostKeyGate();
  const [bindings, setBindings] = useState<Record<string, RemoteAgentBinding>>({});
  const [servers, setServers] = useState<SshServer[]>([]);
  const [projects, setProjects] = useState<RemoteProject[]>([]);
  const [picker, setPicker] = useState<PickerState | null>(null);
  const [starting, setStarting] = useState<StartingState | null>(null);
  const [checking, setChecking] = useState<string | null>(null);
  // Per-agent Open/Close (expand/collapse) — purely local UI state; the
  // binding/session storage is untouched by this toggle.
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const isExpanded = (id: string) => expanded[id] ?? false;
  const toggleExpanded = (id: string) => setExpanded((prev) => ({ ...prev, [id]: !prev[id] }));

  const loadBindings = useCallback(() => {
    const next: Record<string, RemoteAgentBinding> = {};
    for (const agent of ALL_AGENTS) {
      const binding = getRemoteAgentBinding(agent.id);
      if (binding) next[agent.id] = binding;
    }
    setBindings(next);
  }, []);

  useEffect(() => {
    loadBindings();
    void (async () => {
      try {
        const [serverList, projectList] = await Promise.all([
          listSshServers(),
          listRemoteProjects(),
        ]);
        setServers(serverList);
        setProjects(projectList);
      } catch {
        /* dashboard renders without lists; picker shows empty states */
      }
    })();
    return subscribeRemoteAgentBindings(loadBindings);
  }, [loadBindings]);

  const startRemote = useCallback(
    async (agent: AgentConfig, binding: RemoteAgentBinding) => {
      setChecking(agent.id);
      try {
        const server = servers.find((s) => s.id === binding.serverId);
        const project = projects.find((p) => p.id === binding.projectId);
        if (!server || !project) {
          showToast(
            'Bound SSH server or remote project no longer exists. Reconnect the agent.',
            'error'
          );
          return;
        }
        // Explicit host-key gate before the probe (known → proceed, unknown →
        // fingerprint confirmation, changed/unprobeable → block).
        if (!(await ensureHostKeyAccepted(server))) return;
        const status = await checkRemoteAgentInstalled(binding.serverId, agent.binaryName);
        if (!status.installed) {
          showToast(
            `${agent.displayName} is not installed on ${binding.serverName}. Install it on the VPS first.`,
            'error'
          );
          return;
        }
        setStarting({ agent, server, project });
      } catch (err) {
        showToast(formatCommandError(asCommandError(err)), 'error');
      } finally {
        setChecking(null);
      }
    },
    [servers, projects, ensureHostKeyAccepted, showToast]
  );

  const disconnect = (agent: AgentConfig) => {
    const binding = getRemoteAgentBinding(agent.id);
    if (binding) {
      const ptyId = ptySessionIdFor(binding.serverId, agent.id, binding.path);
      void killPtySession(ptyId).catch(() => {
        /* session may already be dead */
      });
    }
    clearRemoteAgentBinding(agent.id);
    showToast(`${agent.displayName} disconnected from remote — returned to LOCAL`, 'info');
  };

  const saveBinding = (agent: AgentConfig, server: SshServer, project: RemoteProject) => {
    setRemoteAgentBinding(agent.id, {
      serverId: server.id,
      serverName: server.name,
      projectId: project.id,
      projectName: project.name,
      path: project.remotePath,
      boundAt: Date.now(),
    });
    setPicker(null);
  };

  return (
    <div style={{ marginBottom: 'var(--spacing-lg)' }}>
      <h3 style={{ margin: 0, fontSize: 'var(--font-size-lg)' }}>Agents</h3>
      <p
        style={{
          margin: '0 0 var(--spacing-md) 0',
          color: 'var(--text-muted)',
          fontSize: 'var(--font-size-sm)',
        }}
      >
        LOCAL runs the agent CLI on this machine (built-in). REMOTE runs it on a VPS over SSH.
      </p>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--spacing-sm)' }}>
        {ALL_AGENTS.map((agent) => {
          const binding = bindings[agent.id];
          const busy = checking === agent.id;
          const open = isExpanded(agent.id);
          const session = binding
            ? getRemoteAgentSessions().find(
                (s) => s.projectId === binding.projectId && s.agentId === agent.id
              )
            : undefined;
          const detailText = binding
            ? `Session: ${session?.status ?? 'not started'} — this agent runs on ${binding.serverName} → ${binding.projectName} (${binding.path}). Click ▲ to collapse.`
            : 'LOCAL (built-in): the agent CLI runs on this machine.';

          return (
            <div key={agent.id} className="ssh-server-card">
              <div className="ssh-server-info">
                <span className="ssh-server-name">{agent.displayName}</span>
                {binding && (
                  <span className="ssh-server-detail">
                    REMOTE — {binding.serverName} → {binding.projectName} ({binding.path})
                  </span>
                )}
                <span
                  className={`ssh-connection-badge ssh-connection-badge--${
                    binding ? 'connected' : 'disconnected'
                  }`}
                >
                  {binding ? 'remote' : 'LOCAL (built-in)'}
                </span>
              </div>
              <div className="ssh-server-actions">
                {binding && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => toggleExpanded(agent.id)}
                    aria-expanded={open}
                    title={open ? 'Collapse' : 'Expand'}
                  >
                    {open ? '▲' : '▼'}
                  </Button>
                )}
                {binding && (
                  <Button
                    variant="primary"
                    size="sm"
                    disabled={busy}
                    onClick={() => {
                      const bindingNow = getRemoteAgentBinding(agent.id);
                      if (bindingNow) void startRemote(agent, bindingNow);
                    }}
                  >
                    {busy ? 'Checking…' : 'Start'}
                  </Button>
                )}
                {binding && (
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() =>
                      setPicker({ agent, serverId: binding.serverId, projectId: binding.projectId })
                    }
                  >
                    Change
                  </Button>
                )}
                {binding && (
                  <Button variant="ghost" size="sm" onClick={() => disconnect(agent)}>
                    Disconnect
                  </Button>
                )}
                {!binding && (
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => setPicker({ agent, serverId: '', projectId: '' })}
                  >
                    Remote…
                  </Button>
                )}
              </div>
              {open && (
                <div
                  style={{
                    flexBasis: '100%',
                    padding: 'var(--spacing-xs) var(--spacing-sm)',
                    color: 'var(--text-muted)',
                    fontSize: 'var(--font-size-sm)',
                  }}
                >
                  {detailText}
                </div>
              )}
            </div>
          );
        })}
      </div>{' '}
      {picker && (
        <ModalFrame
          isOpen
          onClose={() => setPicker(null)}
          title={`Connect ${picker.agent.displayName} — remote`}
        >
          <div
            style={{
              padding: 'var(--spacing-lg)',
              display: 'flex',
              flexDirection: 'column',
              gap: 'var(--spacing-md)',
            }}
          >
            <div className="form-group">
              <label htmlFor="agent-launch-server">SSH server</label>
              <Dropdown
                menuClassName="remote-agent-select-menu"
                trigger={(p) => {
                  const current = servers.find((s) => s.id === picker.serverId);
                  return (
                    <button
                      type="button"
                      className="remote-agent-select-trigger"
                      id="agent-launch-server"
                      {...p}
                    >
                      <span>
                        {current
                          ? `${current.name} (${current.username}@${current.host})`
                          : '— select server —'}
                      </span>
                      <ChevronIcon size={12} />
                    </button>
                  );
                }}
              >
                {servers.map((s) => (
                  <DropdownItem
                    key={s.id}
                    active={s.id === picker.serverId}
                    onSelect={() => setPicker({ ...picker, serverId: s.id, projectId: '' })}
                  >
                    {s.name} ({s.username}@{s.host})
                  </DropdownItem>
                ))}
              </Dropdown>
            </div>
            <div className="form-group">
              <label htmlFor="agent-launch-project">Remote project</label>
              <Dropdown
                menuClassName="remote-agent-select-menu"
                trigger={(p) => {
                  const current = projects.find(
                    (pr) => pr.serverId === picker.serverId && pr.id === picker.projectId
                  );
                  return (
                    <button
                      type="button"
                      className="remote-agent-select-trigger"
                      id="agent-launch-project"
                      disabled={!picker.serverId}
                      {...p}
                    >
                      <span>
                        {current ? `${current.name} (${current.remotePath})` : '— select project —'}
                      </span>
                      <ChevronIcon size={12} />
                    </button>
                  );
                }}
              >
                {projects
                  .filter((p) => p.serverId === picker.serverId)
                  .map((p) => (
                    <DropdownItem
                      key={p.id}
                      active={p.id === picker.projectId}
                      onSelect={() => setPicker({ ...picker, projectId: p.id })}
                    >
                      {p.name} ({p.remotePath})
                    </DropdownItem>
                  ))}
              </Dropdown>
            </div>
            <div className="modal-actions">
              <Button variant="secondary" size="sm" onClick={() => setPicker(null)}>
                Cancel
              </Button>
              <Button
                variant="primary"
                size="sm"
                disabled={!picker.serverId || !picker.projectId}
                onClick={() => {
                  const server = servers.find((s) => s.id === picker.serverId);
                  const project = projects.find((p) => p.id === picker.projectId);
                  if (server && project) {
                    saveBinding(picker.agent, server, project);
                    const bindingNow = getRemoteAgentBinding(picker.agent.id);
                    if (bindingNow) void startRemote(picker.agent, bindingNow);
                  }
                }}
              >
                {checking === picker.agent.id ? 'Checking…' : 'Connect & Start'}
              </Button>
            </div>
          </div>
        </ModalFrame>
      )}
      {starting && (
        <ModalFrame
          isOpen
          onClose={() => setStarting(null)}
          title={`${starting.agent.displayName} — ${starting.server.name} ${starting.project.remotePath}`}
          className="agent-launch-terminal-modal"
          showCloseButton={false}
        >
          <div style={{ height: '70vh', display: 'flex', flexDirection: 'column' }}>
            <RemoteAgentTerminal
              agent={starting.agent}
              server={starting.server}
              remotePath={starting.project.remotePath}
              projectId={starting.project.id}
              projectName={starting.project.name}
              onBack={() => setStarting(null)}
            />
          </div>
        </ModalFrame>
      )}
      {hostKeyModal}
    </div>
  );
}
