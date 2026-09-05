/**
 * Tests for HostKeyConfirmModal:
 * - `unknown` mode shows the fingerprint and offers Trust & connect / Cancel
 * - `changed` mode blocks with NO blind-accept option
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { HostKeyConfirmModal } from './HostKeyConfirmModal';
import type { SshServer, HostKeyStatus } from '../../lib/ssh';

const server: SshServer = {
  id: 'srv-1',
  name: 'Test VPS',
  host: 'example.com',
  port: 22,
  username: 'deploy',
  keyPath: null,
  authType: 'key',
  createdAt: 0,
  lastConnectedAt: null,
};

const unknownStatus: HostKeyStatus = {
  state: 'unknown',
  fingerprint: 'SHA256:AbCdEf123',
  keyType: 'ed25519',
};

const changedStatus: HostKeyStatus = {
  state: 'changed',
  fingerprint: 'SHA256:NeWkEy456',
  keyType: 'ed25519',
};

describe('HostKeyConfirmModal', () => {
  it('unknown mode shows host, fingerprint and both actions', () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <HostKeyConfirmModal
        isOpen
        mode="unknown"
        server={server}
        status={unknownStatus}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />
    );

    expect(screen.getByText('Trust this host?')).toBeInTheDocument();
    expect(screen.getByText('SHA256:AbCdEf123')).toBeInTheDocument();
    expect(screen.getByText('ed25519')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Trust & connect' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument();
  });

  it('confirm resolves the trust flow and cancel aborts it', () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    const { rerender } = render(
      <HostKeyConfirmModal
        isOpen
        mode="unknown"
        server={server}
        status={unknownStatus}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />
    );

    fireEvent.click(screen.getByRole('button', { name: 'Trust & connect' }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(onCancel).not.toHaveBeenCalled();

    rerender(
      <HostKeyConfirmModal
        isOpen
        mode="unknown"
        server={server}
        status={unknownStatus}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />
    );
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it('changed mode blocks: no Trust & connect, warning shown, Close cancels', () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <HostKeyConfirmModal
        isOpen
        mode="changed"
        server={server}
        status={changedStatus}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />
    );

    expect(screen.getByText('Host key changed — connection blocked')).toBeInTheDocument();
    expect(screen.getByText('SHA256:NeWkEy456')).toBeInTheDocument();
    expect(screen.getByText(/man-in-the-middle attack/i)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Trust & connect' })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
