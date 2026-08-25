/**
 * AddServerModal — create or edit an SSH server configuration.
 *
 * Form fields: name, host, port (optional, default 22), username, key path
 * (optional). Uses `<ModalFrame>` and `<Button variant>` per the shared
 * primitives. A "Test" button runs a connection check before saving.
 *
 * @module components/ssh/AddServerModal
 */

import { useState, useRef, useEffect } from 'react';
import { ModalFrame } from '../primitives/ModalFrame';
import { Button } from '../primitives/Button';
import { asCommandError, formatCommandError } from '../../lib/errors';
import { addSshServer, updateSshServer, testSshConnection, type SshServer } from '../../lib/ssh';

interface AddServerModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSaved: () => void;
  /** When set, the modal operates in edit mode with pre-filled values. */
  editServer?: SshServer | null;
}

export function AddServerModal({ isOpen, onClose, onSaved, editServer }: AddServerModalProps) {
  const isEdit = !!editServer;
  const [name, setName] = useState('');
  const [host, setHost] = useState('');
  const [port, setPort] = useState('');
  const [username, setUsername] = useState('');
  const [keyPath, setKeyPath] = useState('');
  const [loading, setLoading] = useState(false);
  const [testing, setTesting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (isOpen) {
      if (editServer) {
        setName(editServer.name);
        setHost(editServer.host);
        setPort(editServer.port ? String(editServer.port) : '');
        setUsername(editServer.username);
        setKeyPath(editServer.keyPath ?? '');
      } else {
        setName('');
        setHost('');
        setPort('');
        setUsername('');
        setKeyPath('');
      }
      setError(null);
      setTestResult(null);
      setLoading(false);
      setTesting(false);
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  }, [isOpen, editServer]);

  const buildConfig = () => ({
    name: name.trim(),
    host: host.trim(),
    port: port.trim() ? Number(port.trim()) : null,
    username: username.trim(),
    keyPath: keyPath.trim() || null,
  });

  const handleTest = async () => {
    setTesting(true);
    setError(null);
    setTestResult(null);
    try {
      let serverId = editServer?.id;
      if (!serverId) {
        const saved = await addSshServer(buildConfig());
        serverId = saved.id;
        setTestResult('Server saved. Testing connection...');
      }
      if (serverId) {
        await testSshConnection(serverId);
        setTestResult('Connection successful!');
      }
    } catch (err) {
      setTestResult(null);
      setError(formatCommandError(asCommandError(err)));
    } finally {
      setTesting(false);
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim() || !host.trim() || !username.trim()) {
      setError('Name, host, and username are required');
      return;
    }
    setLoading(true);
    setError(null);
    try {
      if (isEdit && editServer) {
        await updateSshServer(editServer.id, buildConfig());
      } else {
        await addSshServer(buildConfig());
      }
      onSaved();
      onClose();
    } catch (err) {
      setError(formatCommandError(asCommandError(err)));
    } finally {
      setLoading(false);
    }
  };

  return (
    <ModalFrame
      isOpen={isOpen}
      onClose={onClose}
      title={isEdit ? 'Edit SSH Server' : 'Add SSH Server'}
      dismissable={!loading && !testing}
    >
      <form onSubmit={(e) => void handleSubmit(e)} style={{ padding: 'var(--spacing-xl)' }}>
        <div className="form-group">
          <label htmlFor="ssh-name">Server name</label>
          <input
            ref={inputRef}
            id="ssh-name"
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Production VPS"
            disabled={loading || testing}
            autoComplete="off"
            spellCheck={false}
          />
        </div>
        <div className="form-group">
          <label htmlFor="ssh-host">Host</label>
          <input
            id="ssh-host"
            type="text"
            value={host}
            onChange={(e) => setHost(e.target.value)}
            placeholder="203.0.113.1 or example.com"
            disabled={loading || testing}
            autoComplete="off"
            spellCheck={false}
          />
        </div>
        <div className="form-group">
          <label htmlFor="ssh-port">Port (optional, default 22)</label>
          <input
            id="ssh-port"
            type="number"
            value={port}
            onChange={(e) => setPort(e.target.value)}
            placeholder="22"
            disabled={loading || testing}
            min={1}
            max={65535}
          />
        </div>
        <div className="form-group">
          <label htmlFor="ssh-username">Username</label>
          <input
            id="ssh-username"
            type="text"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            placeholder="root"
            disabled={loading || testing}
            autoComplete="off"
            spellCheck={false}
          />
        </div>
        <div className="form-group">
          <label htmlFor="ssh-key">SSH key path (optional)</label>
          <input
            id="ssh-key"
            type="text"
            value={keyPath}
            onChange={(e) => setKeyPath(e.target.value)}
            placeholder="~/.ssh/id_ed25519"
            disabled={loading || testing}
            autoComplete="off"
            spellCheck={false}
          />
        </div>
        {error && <p className="form-error">{error}</p>}
        {testResult && <p className="ssh-test-result">{testResult}</p>}
        <div className="modal-actions">
          <Button variant="secondary" type="button" onClick={onClose} disabled={loading || testing}>
            Cancel
          </Button>
          <Button
            variant="secondary"
            type="button"
            onClick={() => void handleTest()}
            disabled={testing || loading || !name.trim() || !host.trim() || !username.trim()}
          >
            {testing ? 'Testing...' : 'Test'}
          </Button>
          <Button
            variant="primary"
            type="submit"
            disabled={loading || testing || !name.trim() || !host.trim() || !username.trim()}
          >
            {loading ? 'Saving...' : isEdit ? 'Update' : 'Save'}
          </Button>
        </div>
      </form>
    </ModalFrame>
  );
}
