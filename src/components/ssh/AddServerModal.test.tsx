/**
 * Tests for AddServerModal authentication-mode selection:
 * - key mode (default) shows the key path field and submits authType: key
 * - password mode shows the password field, hides the key path, and passes
 *   the password transiently (IPC → OS keystore; the frontend never stores it)
 * - edit with a blank password keeps the stored one (password: null)
 * - add + password mode without a password is refused before any IPC
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { AddServerModal } from './AddServerModal';
import type { SshServer } from '../../lib/ssh';

vi.mock('../../lib/ssh', async () => {
  const actual = await vi.importActual<typeof import('../../lib/ssh')>('../../lib/ssh');
  return {
    ...actual,
    addSshServer: vi.fn(),
    updateSshServer: vi.fn(),
    testSshConnection: vi.fn(),
  };
});
import { addSshServer, updateSshServer, testSshConnection } from '../../lib/ssh';

const keyServer: SshServer = {
  id: 'srv-key',
  name: 'Key VPS',
  host: 'example.com',
  port: 22,
  username: 'deploy',
  keyPath: '~/.ssh/id_ed25519',
  authType: 'key',
  createdAt: 0,
  lastConnectedAt: null,
};

const passwordServer: SshServer = {
  ...keyServer,
  id: 'srv-pw',
  name: 'Pw VPS',
  authType: 'password',
  keyPath: null,
};

function fillBaseFields() {
  fireEvent.change(screen.getByLabelText('Server name'), { target: { value: 'My VPS' } });
  fireEvent.change(screen.getByLabelText('Host'), { target: { value: '203.0.113.1' } });
  fireEvent.change(screen.getByLabelText('Username'), { target: { value: 'root' } });
}

describe('AddServerModal auth modes', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(addSshServer).mockResolvedValue(keyServer);
    vi.mocked(updateSshServer).mockResolvedValue(keyServer);
    vi.mocked(testSshConnection).mockResolvedValue('ok');
  });

  it('defaults to key mode: key path field visible, authType key submitted', async () => {
    render(<AddServerModal isOpen onClose={() => {}} onSaved={() => {}} />);
    fillBaseFields();
    expect(screen.getByLabelText(/SSH key path/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(addSshServer).toHaveBeenCalledTimes(1));
    expect(addSshServer).toHaveBeenCalledWith(
      expect.objectContaining({ authType: 'key', keyPath: null })
    );
  });

  it('password mode shows the password field and passes it transiently', async () => {
    render(<AddServerModal isOpen onClose={() => {}} onSaved={() => {}} />);
    fillBaseFields();
    fireEvent.click(screen.getByRole('button', { name: 'Password' }));
    expect(screen.queryByLabelText(/SSH key path/)).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'topsecret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(addSshServer).toHaveBeenCalledTimes(1));
    expect(addSshServer).toHaveBeenCalledWith(
      expect.objectContaining({ authType: 'password', password: 'topsecret', keyPath: null })
    );
  });

  it('add + password mode without a password is refused before any IPC', () => {
    render(<AddServerModal isOpen onClose={() => {}} onSaved={() => {}} />);
    fillBaseFields();
    fireEvent.click(screen.getByRole('button', { name: 'Password' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(
      screen.getByText('A password is required for password authentication')
    ).toBeInTheDocument();
    expect(addSshServer).not.toHaveBeenCalled();
  });

  it('edit with a blank password keeps the stored one (password: null)', async () => {
    render(
      <AddServerModal isOpen onClose={() => {}} onSaved={() => {}} editServer={passwordServer} />
    );
    fireEvent.click(screen.getByRole('button', { name: 'Update' }));

    await waitFor(() => expect(updateSshServer).toHaveBeenCalledTimes(1));
    expect(updateSshServer).toHaveBeenCalledWith(
      'srv-pw',
      expect.objectContaining({ authType: 'password', password: null })
    );
  });

  it('edit key server keeps key mode with its key path', async () => {
    render(<AddServerModal isOpen onClose={() => {}} onSaved={() => {}} editServer={keyServer} />);
    expect(screen.getByLabelText(/SSH key path/)).toHaveValue('~/.ssh/id_ed25519');
    fireEvent.click(screen.getByRole('button', { name: 'Update' }));

    await waitFor(() => expect(updateSshServer).toHaveBeenCalledTimes(1));
    expect(updateSshServer).toHaveBeenCalledWith(
      'srv-key',
      expect.objectContaining({ authType: 'key', keyPath: '~/.ssh/id_ed25519' })
    );
  });

  it('edit mode: Test persists the typed auth change BEFORE testing', async () => {
    render(<AddServerModal isOpen onClose={() => {}} onSaved={() => {}} editServer={keyServer} />);
    // Switch the saved key server to password mode, type the password, then
    // click Test (not Update) — the auth change must reach the backend
    // (keystore + config) BEFORE the connection attempt runs.
    fireEvent.click(screen.getByRole('button', { name: 'Password' }));
    fireEvent.change(screen.getByLabelText(/Password/), { target: { value: 'topsecret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Test' }));

    await waitFor(() => expect(testSshConnection).toHaveBeenCalledWith('srv-key'));
    expect(updateSshServer).toHaveBeenCalledWith(
      'srv-key',
      expect.objectContaining({ authType: 'password', password: 'topsecret' })
    );
    // Order guard: persist before test.
    expect(vi.mocked(updateSshServer).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(testSshConnection).mock.invocationCallOrder[0]
    );
  });
});
