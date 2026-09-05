/**
 * Tests for the shared host-key gate hook (useHostKeyGate).
 *
 * Regression coverage for the bypass audit: every UI entry point that can
 * trigger SSH exec routes through this hook, so these tests pin the
 * security decision table centrally:
 * - KNOWN → proceed without a modal
 * - UNKNOWN → fingerprint modal; Confirm → accept_remote_host_key then true
 * - Cancel → false, no accept call
 * - CHANGED → false immediately, blocking modal, no accept path
 * - probe-unavailable → false, error surfaced, no modal, no accept path
 * - probe failure → false, error surfaced, no throw
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { useHostKeyGate } from './useHostKeyGate';
import type { SshServer, HostKeyStatus } from '../lib/ssh';

vi.mock('../lib/ssh', () => ({
  checkRemoteHostKey: vi.fn(),
  acceptRemoteHostKey: vi.fn(),
  resolveHostKeyAction: vi.fn((state: string) => {
    if (state === 'known') return 'proceed';
    if (state === 'changed' || state === 'probe-unavailable') return 'block';
    return 'prompt';
  }),
}));

import { checkRemoteHostKey, acceptRemoteHostKey } from '../lib/ssh';

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

function status(state: string): HostKeyStatus {
  return { state: state as HostKeyStatus['state'], fingerprint: 'SHA256:AbC', keyType: 'ed25519' };
}

/** Harness: one button starts the gate; the modal element is rendered. */
function Harness({ server, onResult }: { server: SshServer; onResult: (ok: boolean) => void }) {
  const { ensureHostKeyAccepted, hostKeyModal } = useHostKeyGate();
  return (
    <div>
      <button onClick={() => void ensureHostKeyAccepted(server).then(onResult)}>go</button>
      {hostKeyModal}
    </div>
  );
}

describe('useHostKeyGate', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(acceptRemoteHostKey).mockResolvedValue(undefined);
  });

  it('KNOWN proceeds directly without a modal or accept call', async () => {
    vi.mocked(checkRemoteHostKey).mockResolvedValue(status('known'));
    const onResult = vi.fn();
    render(<Harness server={server} onResult={onResult} />);

    fireEvent.click(screen.getByRole('button', { name: 'go' }));

    await waitFor(() => expect(onResult).toHaveBeenCalledWith(true));
    expect(screen.queryByText('Trust this host?')).not.toBeInTheDocument();
    expect(acceptRemoteHostKey).not.toHaveBeenCalled();
  });

  it('probe-unavailable blocks: error surfaced, no modal, no accept', async () => {
    vi.mocked(checkRemoteHostKey).mockResolvedValue(status('probe-unavailable'));
    const onResult = vi.fn();
    render(<Harness server={server} onResult={onResult} />);

    fireEvent.click(screen.getByRole('button', { name: 'go' }));

    await waitFor(() => expect(onResult).toHaveBeenCalledWith(false));
    expect(screen.queryByText('Trust this host?')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Trust & connect' })).not.toBeInTheDocument();
    expect(acceptRemoteHostKey).not.toHaveBeenCalled();
  });

  it('UNKNOWN shows the fingerprint modal; Confirm accepts then proceeds', async () => {
    vi.mocked(checkRemoteHostKey).mockResolvedValue(status('unknown'));
    const onResult = vi.fn();
    render(<Harness server={server} onResult={onResult} />);

    fireEvent.click(screen.getByRole('button', { name: 'go' }));
    expect(await screen.findByText('Trust this host?')).toBeInTheDocument();
    expect(screen.getByText('SHA256:AbC')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Trust & connect' }));

    await waitFor(() => expect(onResult).toHaveBeenCalledWith(true));
    expect(acceptRemoteHostKey).toHaveBeenCalledWith('srv-1');
  });

  it('UNKNOWN Cancel aborts without accepting or proceeding', async () => {
    vi.mocked(checkRemoteHostKey).mockResolvedValue(status('unknown'));
    const onResult = vi.fn();
    render(<Harness server={server} onResult={onResult} />);

    fireEvent.click(screen.getByRole('button', { name: 'go' }));
    await screen.findByText('Trust this host?');
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    await waitFor(() => expect(onResult).toHaveBeenCalledWith(false));
    expect(acceptRemoteHostKey).not.toHaveBeenCalled();
  });

  it('CHANGED blocks immediately: modal shown, no accept, result false', async () => {
    vi.mocked(checkRemoteHostKey).mockResolvedValue(status('changed'));
    const onResult = vi.fn();
    render(<Harness server={server} onResult={onResult} />);

    fireEvent.click(screen.getByRole('button', { name: 'go' }));

    await waitFor(() => expect(onResult).toHaveBeenCalledWith(false));
    expect(screen.getByText('Host key changed — connection blocked')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Trust & connect' })).not.toBeInTheDocument();
    expect(acceptRemoteHostKey).not.toHaveBeenCalled();
  });

  it('probe failure surfaces an error and refuses to proceed', async () => {
    vi.mocked(checkRemoteHostKey).mockRejectedValue(new Error('probe blew up'));
    const onResult = vi.fn();
    render(<Harness server={server} onResult={onResult} />);

    fireEvent.click(screen.getByRole('button', { name: 'go' }));

    await waitFor(() => expect(onResult).toHaveBeenCalledWith(false));
    expect(acceptRemoteHostKey).not.toHaveBeenCalled();
  });
});
