/**
 * HostKeyConfirmModal — explicit host-key trust confirmation.
 *
 * Shown by ServerList before the first connection to a server:
 * - `unknown` mode: displays the probed fingerprint and asks the user to
 *   trust it ("Trust & connect" / "Cancel").
 * - `changed` mode: the stored known_hosts key no longer matches the probed
 *   key — the connection is BLOCKED with no blind-accept option.
 *
 * @module components/ssh/HostKeyConfirmModal
 */

import { ModalFrame } from '../primitives/ModalFrame';
import { Button } from '../primitives/Button';
import type { SshServer, HostKeyStatus } from '../../lib/ssh';

interface HostKeyConfirmModalProps {
  isOpen: boolean;
  mode: 'unknown' | 'changed';
  server: SshServer | null;
  status: HostKeyStatus | null;
  onConfirm: () => void;
  onCancel: () => void;
}

export function HostKeyConfirmModal({
  isOpen,
  mode,
  server,
  status,
  onConfirm,
  onCancel,
}: HostKeyConfirmModalProps) {
  const changed = mode === 'changed';
  return (
    <ModalFrame
      isOpen={isOpen}
      onClose={onCancel}
      title={changed ? 'Host key changed — connection blocked' : 'Trust this host?'}
    >
      {server && (
        <div className="ssh-remote-files" style={{ gap: 'var(--spacing-sm)' }}>
          <p style={{ margin: 0, fontSize: 'var(--font-size-sm)' }}>
            {server.name} — {server.username}@{server.host}
            {server.port && server.port !== 22 ? `:${server.port}` : ''}
          </p>

          {changed ? (
            <p style={{ margin: 0, fontSize: 'var(--font-size-sm)', color: 'var(--error)' }}>
              The key presented by this host differs from the one previously stored in your
              known_hosts. This can indicate a man-in-the-middle attack or a server rebuild. The
              connection is blocked. Verify the new key out-of-band, then remove the stale entry
              from your known_hosts manually before reconnecting.
            </p>
          ) : (
            <p
              style={{ margin: 0, fontSize: 'var(--font-size-sm)', color: 'var(--text-secondary)' }}
            >
              This is the first connection to this host. Verify the fingerprint below against what
              your server operator published before trusting it.
            </p>
          )}

          {status?.fingerprint && (
            <div className="ssh-server-card" style={{ cursor: 'default' }}>
              <div className="ssh-server-info">
                <span className="ssh-server-name">{status.fingerprint}</span>
                {status.keyType && <span className="ssh-server-detail">{status.keyType}</span>}
              </div>
            </div>
          )}

          <div style={{ display: 'flex', gap: 'var(--spacing-sm)', justifyContent: 'flex-end' }}>
            <Button variant="secondary" size="sm" onClick={onCancel}>
              {changed ? 'Close' : 'Cancel'}
            </Button>
            {!changed && (
              <Button variant="primary" size="sm" onClick={onConfirm}>
                Trust &amp; connect
              </Button>
            )}
          </div>
        </div>
      )}
    </ModalFrame>
  );
}
