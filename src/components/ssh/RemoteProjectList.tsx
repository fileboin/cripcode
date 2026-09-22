/**
 * RemoteProjectList — displays registered remote projects on the dashboard.
 * Each card shows the project name, server info, and remote path. Clicking
 * a project opens the remote file browser for that VPS path.
 *
 * @module components/ssh/RemoteProjectList
 */

import { useState, useCallback } from 'react';
import { Button } from '../primitives/Button';
import { Spinner } from '../primitives/Spinner';
import { useAsyncState } from '../../hooks/useAsyncState';
import { useHostKeyGate } from '../../hooks/useHostKeyGate';
import { useOptionalToast } from '../../contexts/ToastContext';
import { asCommandError, formatCommandError } from '../../lib/errors';
import {
  listRemoteProjects,
  removeRemoteProject,
  markRemoteProjectOpened,
  setRemoteProjectLocalLink,
  type RemoteProject,
} from '../../lib/remoteProjects';
import { listSshServers, type SshServer } from '../../lib/ssh';
import { listProjects } from '../../lib/project';
import { isAndroid } from '../../lib/setup';
import { useModal } from '../../contexts/ModalContext';
import { RemoteFileBrowser } from './RemoteFileBrowser';
import { AddRemoteProjectModal } from './AddRemoteProjectModal';
import { logger } from '../../lib/logger';

interface RemoteProjectListProps {
  /** Called when a remote project is opened — the parent can switch views. */
  onOpenRemoteProject?: (project: RemoteProject, server: SshServer) => void;
}

export function RemoteProjectList({ onOpenRemoteProject }: RemoteProjectListProps) {
  const { showToast } = useOptionalToast();
  const { ensureHostKeyAccepted, hostKeyModal } = useHostKeyGate();
  const sshServers = useModal('sshServers');
  const [projects, setProjects] = useState<RemoteProject[]>([]);
  const [servers, setServers] = useState<SshServer[]>([]);
  const [localProjects, setLocalProjects] = useState<{ name: string; path: string }[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [addModalOpen, setAddModalOpen] = useState(false);
  const [openProject, setOpenProject] = useState<{
    project: RemoteProject;
    server: SshServer;
  } | null>(null);

  const loadProjects = useCallback(async () => {
    try {
      const [projList, serverList] = await Promise.all([listRemoteProjects(), listSshServers()]);
      setProjects(projList);
      setServers(serverList);
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    } finally {
      setIsLoading(false);
    }
  }, [showToast]);

  // Local repos are only needed for the optional "link" dropdown; never block
  // the list on their failure.
  useAsyncState(
    useCallback(async () => {
      try {
        setLocalProjects(await listProjects());
      } catch {
        setLocalProjects([]);
      }
    }, []),
    { immediate: true }
  );

  useAsyncState(loadProjects, { immediate: true });

  const handleLinkChange = async (project: RemoteProject, localProjectPath: string) => {
    try {
      const updated = await setRemoteProjectLocalLink(project.id, localProjectPath || null);
      setProjects((prev) => prev.map((p) => (p.id === updated.id ? updated : p)));
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    }
  };

  const handleOpen = async (project: RemoteProject) => {
    logger.info('[RemoteProjectList] open clicked', { name: project.name });
    const server = servers.find((s) => s.id === project.serverId);
    if (!server) {
      showToast('SSH server not found for this project', 'error');
      return;
    }
    if (!(await ensureHostKeyAccepted(server))) return;
    try {
      await markRemoteProjectOpened(project.id);
      if (onOpenRemoteProject) {
        onOpenRemoteProject(project, server);
      } else {
        setOpenProject({ project, server });
      }
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    }
  };

  const handleRemove = async (project: RemoteProject) => {
    if (!window.confirm(`Remove "${project.name}"? (Files on the VPS are NOT deleted.)`)) return;
    try {
      await removeRemoteProject(project.id);
      await loadProjects();
      showToast('Remote project removed', 'info');
    } catch (err) {
      showToast(formatCommandError(asCommandError(err)), 'error');
    }
  };

  const handleAdded = () => {
    void loadProjects();
  };

  if (openProject) {
    return (
      <RemoteFileBrowser
        server={openProject.server}
        projectId={openProject.project.id}
        projectName={openProject.project.name}
        initialPath={openProject.project.remotePath}
        onBack={() => {
          setOpenProject(null);
          void loadProjects();
        }}
      />
    );
  }

  if (isLoading) {
    return (
      <div style={{ padding: 'var(--spacing-lg)', textAlign: 'center' }}>
        <Spinner size="sm" />
      </div>
    );
  }

  return (
    <div className="ssh-remote-project-section">
      {hostKeyModal}
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          marginBottom: 'var(--spacing-sm)',
        }}
      >
        <h3
          style={{
            margin: 0,
            fontSize: 'var(--font-size-base)',
            color: 'var(--text-secondary)',
          }}
        >
          Remote Projects
        </h3>
        <div style={{ display: 'flex', gap: 'var(--spacing-sm)' }}>
          {isAndroid() && (
            <Button variant="secondary" size="sm" onClick={sshServers.open}>
              SSH Servers
            </Button>
          )}
          <Button variant="secondary" size="sm" onClick={() => setAddModalOpen(true)}>
            Add Remote Project
          </Button>
        </div>
      </div>

      {projects.length === 0 ? (
        <p
          style={{
            padding: 'var(--spacing-md)',
            fontSize: 'var(--font-size-sm)',
            color: 'var(--text-muted)',
          }}
        >
          No remote projects. Click "Add Remote Project" to connect to a VPS project.
        </p>
      ) : (
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: 'var(--spacing-xs)',
          }}
        >
          {projects.map((project) => {
            const server = servers.find((s) => s.id === project.serverId);
            return (
              <div key={project.id} className="ssh-server-card">
                <div className="ssh-server-info">
                  <span className="ssh-server-name">{project.name}</span>
                  <span className="ssh-server-detail">{project.remotePath}</span>
                  {server && (
                    <span className="ssh-server-detail">
                      {server.username}@{server.host}
                    </span>
                  )}
                  <label
                    className="ssh-server-detail"
                    style={{ display: 'flex', alignItems: 'center', gap: 'var(--spacing-xs)' }}
                  >
                    Local repo:
                    <select
                      value={project.localProjectPath ?? ''}
                      onChange={(e) => void handleLinkChange(project, e.target.value)}
                      aria-label={`Local project linked to ${project.name}`}
                    >
                      <option value="">None</option>
                      {localProjects.map((p) => (
                        <option key={p.path} value={p.path}>
                          {p.name} — {p.path}
                        </option>
                      ))}
                    </select>
                  </label>
                  <span className="ssh-connection-badge ssh-connection-badge--disconnected">
                    remote
                  </span>
                </div>
                <div className="ssh-server-actions">
                  <Button variant="ghost" size="sm" onClick={() => void handleOpen(project)}>
                    Open
                  </Button>
                  <Button variant="ghost" size="sm" onClick={() => void handleRemove(project)}>
                    Remove
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <AddRemoteProjectModal
        isOpen={addModalOpen}
        onClose={() => setAddModalOpen(false)}
        onAdded={handleAdded}
      />
    </div>
  );
}
