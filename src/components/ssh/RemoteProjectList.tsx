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
  type RemoteProject,
} from '../../lib/remoteProjects';
import { listSshServers, type SshServer } from '../../lib/ssh';
import { RemoteFileBrowser } from './RemoteFileBrowser';
import { AddRemoteProjectModal } from './AddRemoteProjectModal';

interface RemoteProjectListProps {
  /** Called when a remote project is opened — the parent can switch views. */
  onOpenRemoteProject?: (project: RemoteProject, server: SshServer) => void;
}

export function RemoteProjectList({ onOpenRemoteProject }: RemoteProjectListProps) {
  const { showToast } = useOptionalToast();
  const { ensureHostKeyAccepted, hostKeyModal } = useHostKeyGate();
  const [projects, setProjects] = useState<RemoteProject[]>([]);
  const [servers, setServers] = useState<SshServer[]>([]);
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

  useAsyncState(loadProjects, { immediate: true });

  const handleOpen = async (project: RemoteProject) => {
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
        <Button variant="secondary" size="sm" onClick={() => setAddModalOpen(true)}>
          Add Remote Project
        </Button>
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
