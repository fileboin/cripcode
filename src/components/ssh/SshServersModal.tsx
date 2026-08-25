/**
 * SshServersModal — wraps the SSH ServerList in a ModalFrame so it can be
 * opened from the command palette or dashboard.
 *
 * @module components/ssh/SshServersModal
 */

import { ModalFrame } from '../primitives/ModalFrame';
import { ServerList } from './ServerList';

interface SshServersModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export function SshServersModal({ isOpen, onClose }: SshServersModalProps) {
  return (
    <ModalFrame isOpen={isOpen} onClose={onClose} title="SSH Servers" className="settings-modal">
      <div className="settings-modal-body">
        <ServerList />
      </div>
    </ModalFrame>
  );
}
