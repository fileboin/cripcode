/**
 * useHostKeyGate — shared explicit host-key confirmation gate.
 *
 * Every UI entry point that can trigger an SSH connection/exec must call
 * `ensureHostKeyAccepted(server)` first and render the returned
 * `hostKeyModal`. The gate:
 * - known → proceed (no modal; ssh still verifies the key at connect time)
 * - probe-unavailable → refuse with a clear error (no silent TOFU)
 * - changed → show the blocking modal and refuse (no blind accept)
 * - unknown → show the fingerprint confirmation; on "Trust & connect"
 *   record the key via `accept_remote_host_key`, then proceed
 *
 * One hook instance per mounting surface (ServerList, RemoteProjectList,
 * passed into AddServerModal as a callback) — the security logic lives here
 * and is unit-tested, so call sites stay thin.
 *
 * @module hooks/useHostKeyGate
 */

import { useState, useCallback, useRef } from 'react';
import { HostKeyConfirmModal } from '../components/ssh/HostKeyConfirmModal';
import {
  checkRemoteHostKey,
  acceptRemoteHostKey,
  resolveHostKeyAction,
  type SshServer,
  type HostKeyStatus,
} from '../lib/ssh';
import { asCommandError, formatCommandError } from '../lib/errors';
import { useOptionalToast } from '../contexts/ToastContext';

export function useHostKeyGate() {
  const { showToast } = useOptionalToast();
  const [hostKeyCheck, setHostKeyCheck] = useState<{
    server: SshServer;
    status: HostKeyStatus;
  } | null>(null);
  const hostKeyResolveRef = useRef<((accepted: boolean) => void) | null>(null);

  const settle = useCallback((accepted: boolean) => {
    const resolve = hostKeyResolveRef.current;
    hostKeyResolveRef.current = null;
    setHostKeyCheck(null);
    resolve?.(accepted);
  }, []);

  const ensureHostKeyAccepted = useCallback(
    async (server: SshServer): Promise<boolean> => {
      let status: HostKeyStatus;
      try {
        status = await checkRemoteHostKey(server.id);
      } catch (err) {
        showToast(formatCommandError(asCommandError(err)), 'error');
        return false;
      }
      if (status.state === 'probe-unavailable') {
        // Fail closed: without a successful probe there is no fingerprint to
        // confirm, and ssh's accept-new would silently trust the host. Never
        // start the connection in this state.
        showToast(
          'Host key verification is unavailable — the host could not be probed. The connection was not started.',
          'error'
        );
        return false;
      }
      const action = resolveHostKeyAction(status.state);
      if (action === 'proceed') return true;
      if (action === 'block') {
        setHostKeyCheck({ server, status });
        return false;
      }
      const accepted = await new Promise<boolean>((resolve) => {
        hostKeyResolveRef.current = resolve;
        setHostKeyCheck({ server, status });
      });
      if (!accepted) return false;
      try {
        await acceptRemoteHostKey(server.id);
        return true;
      } catch (err) {
        showToast(formatCommandError(asCommandError(err)), 'error');
        return false;
      }
    },
    // `settle` is a stable useCallback([]) — provably stable, so not listed.
    [showToast]
  );

  const hostKeyModal = (
    <HostKeyConfirmModal
      isOpen={!!hostKeyCheck}
      mode={hostKeyCheck?.status.state === 'changed' ? 'changed' : 'unknown'}
      server={hostKeyCheck?.server ?? null}
      status={hostKeyCheck?.status ?? null}
      onConfirm={() => settle(true)}
      onCancel={() => settle(false)}
    />
  );

  return { ensureHostKeyAccepted, hostKeyModal };
}
