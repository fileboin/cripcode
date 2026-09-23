/**
 * Tests for AndroidUpdateBanner: versionCode-gated prompt, deferral, and the
 * verified download → installer handoff. The Rust side owns SHA-256 rejection
 * and the trusted-source checks (unit-tested there).
 *
 * @module components/AndroidUpdateBanner.test
 */

import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AndroidUpdateBanner } from './AndroidUpdateBanner';
import * as androidUpdate from '../lib/androidUpdate';

vi.mock('../lib/androidUpdate', () => ({
  checkAndroidUpdate: vi.fn(),
  downloadAndroidUpdate: vi.fn(),
  installAndroidUpdate: vi.fn(),
}));

const mocked = vi.mocked(androidUpdate);

function status(
  overrides: Partial<androidUpdate.AndroidUpdateStatus> = {}
): androidUpdate.AndroidUpdateStatus {
  return {
    available: true,
    localVersionCode: 18008,
    versionName: '0.18.11',
    versionCode: 18009,
    apkUrl:
      'https://gitlab.com/api/v4/projects/eurokrater%2Fcripcode/packages/generic/cripcode/0.18.11/CripCode-0.18.11.apk',
    sha256: 'a'.repeat(64),
    notes: null,
    error: null,
    ...overrides,
  };
}

describe('AndroidUpdateBanner', () => {
  beforeEach(() => {
    sessionStorage.clear();
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('shows the prompt with the new version when the remote versionCode is higher', async () => {
    mocked.checkAndroidUpdate.mockResolvedValue(status());
    render(<AndroidUpdateBanner checkDelayMs={0} />);

    expect(await screen.findByText(/New update available/i)).toBeInTheDocument();
    expect(screen.getByText('v0.18.11')).toBeInTheDocument();
  });

  it('renders nothing when the remote versionCode is equal or lower', async () => {
    mocked.checkAndroidUpdate.mockResolvedValue(
      status({ available: false, versionName: null, versionCode: null })
    );
    render(<AndroidUpdateBanner checkDelayMs={0} />);

    await waitFor(() => expect(mocked.checkAndroidUpdate).toHaveBeenCalled());
    expect(screen.queryByText(/New update available/i)).not.toBeInTheDocument();
  });

  it('renders nothing when the check fails (offline / metadata unavailable)', async () => {
    mocked.checkAndroidUpdate.mockRejectedValue(new Error('offline'));
    render(<AndroidUpdateBanner checkDelayMs={0} />);

    await waitFor(() => expect(mocked.checkAndroidUpdate).toHaveBeenCalled());
    expect(screen.queryByText(/New update available/i)).not.toBeInTheDocument();
  });

  it('Later defers that versionCode for the session', async () => {
    mocked.checkAndroidUpdate.mockResolvedValue(status());
    render(<AndroidUpdateBanner checkDelayMs={0} />);

    fireEvent.click(await screen.findByRole('button', { name: 'Later' }));
    await waitFor(() =>
      expect(screen.queryByText(/New update available/i)).not.toBeInTheDocument()
    );
    expect(sessionStorage.getItem('shipstudio_deferred_android_update')).toBe('18009');
  });

  it('Update downloads the verified APK and hands it to the Android installer', async () => {
    mocked.checkAndroidUpdate.mockResolvedValue(status());
    mocked.downloadAndroidUpdate.mockResolvedValue('/cache/CripCode-update.apk');
    mocked.installAndroidUpdate.mockResolvedValue(undefined);
    render(<AndroidUpdateBanner checkDelayMs={0} />);

    fireEvent.click(await screen.findByRole('button', { name: 'Update' }));
    await waitFor(() =>
      expect(mocked.downloadAndroidUpdate).toHaveBeenCalledWith(status().apkUrl, status().sha256)
    );
    expect(mocked.installAndroidUpdate).toHaveBeenCalledWith('/cache/CripCode-update.apk');
  });

  it('a failed SHA verification never reaches the installer', async () => {
    mocked.checkAndroidUpdate.mockResolvedValue(status());
    mocked.downloadAndroidUpdate.mockRejectedValue(
      new Error('Update rejected: the downloaded file does not match the published SHA-256.')
    );
    render(<AndroidUpdateBanner checkDelayMs={0} />);

    fireEvent.click(await screen.findByRole('button', { name: 'Update' }));
    expect(await screen.findByText(/does not match the published SHA-256/i)).toBeInTheDocument();
    expect(mocked.installAndroidUpdate).not.toHaveBeenCalled();
  });
});
