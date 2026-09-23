/**
 * AndroidUpdateBanner — in-app Android update prompt.
 *
 * Android has no Tauri updater support, so this banner uses the dedicated
 * Rust update commands: numeric versionCode check, verified download, then
 * Android's own installer UI. Rendered by UpdateBanner only on Android;
 * desktop keeps the existing Tauri updater flow untouched.
 *
 * @module components/AndroidUpdateBanner
 */

import { useCallback, useEffect, useState } from 'react';
import { Button } from './primitives/Button';
import { usePolling } from '../hooks/usePolling';
import { asCommandError, formatCommandError } from '../lib/errors';
import {
  checkAndroidUpdate,
  downloadAndroidUpdate,
  installAndroidUpdate,
  type AndroidUpdateStatus,
} from '../lib/androidUpdate';

/** Delay the first check so startup is never blocked. */
const CHECK_DELAY_MS = 5000;
/** Re-check hourly while the app stays open. */
const CHECK_INTERVAL_MS = 60 * 60 * 1000;
/** Session deferral key (per versionCode). */
const DEFERRED_KEY = 'shipstudio_deferred_android_update';

type Phase = 'idle' | 'downloading' | 'installing' | 'error';

interface AndroidUpdateBannerProps {
  /** Delay before the first check (startup must never be blocked). */
  checkDelayMs?: number;
  /** Re-check interval while the app stays open. */
  checkIntervalMs?: number;
}

export function AndroidUpdateBanner({
  checkDelayMs = CHECK_DELAY_MS,
  checkIntervalMs = CHECK_INTERVAL_MS,
}: AndroidUpdateBannerProps = {}) {
  const [status, setStatus] = useState<AndroidUpdateStatus | null>(null);
  const [phase, setPhase] = useState<Phase>('idle');
  const [error, setError] = useState<string | null>(null);
  const [deferred, setDeferred] = useState(false);
  const [checking, setChecking] = useState(false);

  // Delay the first check so startup is never blocked; usePolling then keeps
  // re-checking on the configured interval until unmount.
  useEffect(() => {
    const initial = setTimeout(() => setChecking(true), checkDelayMs);
    return () => clearTimeout(initial);
  }, [checkDelayMs]);

  const runCheck = useCallback(async () => {
    try {
      const next = await checkAndroidUpdate();
      if (!next.available) return;
      setDeferred(sessionStorage.getItem(DEFERRED_KEY) === String(next.versionCode));
      setStatus(next);
    } catch {
      // Offline or endpoint unavailable — treat as "no update" and move on.
    }
  }, []);

  usePolling(runCheck, {
    intervalMs: checkIntervalMs,
    enabled: checking,
    name: 'android-update-check',
  });

  const handleUpdate = useCallback(async () => {
    if (!status?.apkUrl || !status.sha256) return;
    setPhase('downloading');
    setError(null);
    try {
      const path = await downloadAndroidUpdate(status.apkUrl, status.sha256);
      setPhase('installing');
      await installAndroidUpdate(path);
      // The Android installer is now in front of the user; keep the banner
      // until the update actually lands.
      setPhase('idle');
    } catch (err) {
      setError(formatCommandError(asCommandError(err)));
      setPhase('error');
    }
  }, [status]);

  const handleLater = useCallback(() => {
    if (status?.versionCode != null) {
      sessionStorage.setItem(DEFERRED_KEY, String(status.versionCode));
    }
    setDeferred(true);
  }, [status]);

  if (!status?.available || deferred) return null;

  return (
    <div className="update-banner">
      <div className="update-banner-header">
        <div className="update-banner-title">
          <span className="update-banner-badge">New update available</span>
          <span className="update-banner-version">v{status.versionName}</span>
        </div>
        {phase === 'idle' && (
          <div className="update-banner-actions">
            <Button variant="secondary" size="sm" onClick={handleLater}>
              Later
            </Button>
            <Button variant="primary" size="sm" onClick={() => void handleUpdate()}>
              Update
            </Button>
          </div>
        )}
        {phase === 'downloading' && (
          <span className="update-banner-progress-text">Downloading update…</span>
        )}
        {phase === 'installing' && (
          <span className="update-banner-progress-text">Opening Android installer…</span>
        )}
        {phase === 'error' && (
          <div className="update-banner-actions">
            <span className="update-banner-error">{error}</span>
            <Button variant="secondary" size="sm" onClick={() => void handleUpdate()}>
              Retry
            </Button>
          </div>
        )}
      </div>
      {status.notes && phase === 'idle' && (
        <ul className="update-banner-notes">
          <li>{status.notes}</li>
        </ul>
      )}
    </div>
  );
}
