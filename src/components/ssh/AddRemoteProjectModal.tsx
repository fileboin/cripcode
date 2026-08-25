/**
 * AddRemoteProjectModal — register a remote project by picking an SSH server
 * and entering the remote project path.
 *
 * Uses `<ModalFrame>` and `<Button variant>` per the shared primitives.
 *
 * @module components/ssh/AddRemoteProjectModal
 */

import { useState, useRef, useEffect } from 'react';
import { ModalFrame } from '../primitives/ModalFrame';
import { Button } from '../primitives/Button';
import { asCommandError, formatCommandError } from '../../lib/errors';
import { listSshServers, type SshServer } from '../../lib/ssh';
import { addRemoteProject } from '../../lib/remoteProjects';

interface AddRemoteProjectModalProps {
  isOpen: boolean;
  onClose: () => void;
  onAdded: () => void;
}

export function AddRemoteProjectModal({ isOpen, onClose, onAdded }: AddRemoteProjectModalProps) {
  const [servers, setServers] = useState<SshServer[]>([]);
  const [selectedServerId, setSelectedServerId] = useState('');
  const [remotePath, setRemotePath] = useState('');
  const [projectName, setProjectName] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (isOpen) {
      setRemotePath('');
      setProjectName('');
      setError(null);
      setLoading(false);
      void listSshServers().then((list) => {
        setServers(list);
        if (list.length > 0 && !selectedServerId) {
          setSelectedServerId(list[0].id);
        }
      });
      setTimeout(() => inputRef.current?.focus(), 50);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!projectName.trim() || !selectedServerId || !remotePath.trim()) {
      setError('All fields are required');
      return;
    }
    setLoading(true);
    setError(null);
    try {
      await addRemoteProject(projectName.trim(), selectedServerId, remotePath.trim());
      onAdded();
      onClose();
    } catch (err) {
      setError(formatCommandError(asCommandError(err)));
    } finally {
      setLoading(false);
    }
  };

  return (
    <ModalFrame isOpen={isOpen} onClose={onClose} title="Add Remote Project" dismissable={!loading}>
      <form onSubmit={(e) => void handleSubmit(e)} style={{ padding: 'var(--spacing-xl)' }}>
        {servers.length === 0 ? (
          <p className="form-error">
            No SSH servers configured. Add a server first in SSH Servers settings.
          </p>
        ) : (
          <>
            <div className="form-group">
              <label htmlFor="remote-project-name">Project name</label>
              <input
                ref={inputRef}
                id="remote-project-name"
                type="text"
                value={projectName}
                onChange={(e) => setProjectName(e.target.value)}
                placeholder="My Next.js App"
                disabled={loading}
                autoComplete="off"
                spellCheck={false}
              />
            </div>
            <div className="form-group">
              <label htmlFor="remote-project-server">SSH server</label>
              <select
                id="remote-project-server"
                value={selectedServerId}
                onChange={(e) => setSelectedServerId(e.target.value)}
                disabled={loading}
              >
                {servers.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name} — {s.username}@{s.host}
                  </option>
                ))}
              </select>
            </div>
            <div className="form-group">
              <label htmlFor="remote-project-path">Remote project path</label>
              <input
                id="remote-project-path"
                type="text"
                value={remotePath}
                onChange={(e) => setRemotePath(e.target.value)}
                placeholder="/home/user/my-app"
                disabled={loading}
                autoComplete="off"
                spellCheck={false}
              />
            </div>
          </>
        )}
        {error && <p className="form-error">{error}</p>}
        <div className="modal-actions">
          <Button variant="secondary" type="button" onClick={onClose} disabled={loading}>
            Cancel
          </Button>
          <Button
            variant="primary"
            type="submit"
            disabled={
              loading ||
              !projectName.trim() ||
              !selectedServerId ||
              !remotePath.trim() ||
              servers.length === 0
            }
          >
            {loading ? 'Adding...' : 'Add Project'}
          </Button>
        </div>
      </form>
    </ModalFrame>
  );
}
