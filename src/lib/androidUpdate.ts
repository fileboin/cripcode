/**
 * Android in-app update: frontend wrappers around the Rust commands.
 *
 * The metadata source and APK origin are fixed and validated on the Rust side
 * (`commands/android_update.rs`) — nothing here accepts a user-supplied URL.
 * Android shows its own installer confirmation; nothing is ever silent.
 *
 * @module lib/androidUpdate
 */

import { invoke } from '@tauri-apps/api/core';

/** Result of an update check. `error` is set when the check itself failed. */
export interface AndroidUpdateStatus {
  available: boolean;
  /** versionCode of the running app (numeric — not the release label). */
  localVersionCode: number;
  versionName: string | null;
  versionCode: number | null;
  apkUrl: string | null;
  sha256: string | null;
  notes: string | null;
  error: string | null;
}

/** Check the trusted metadata endpoint and compare numeric versionCodes. */
export async function checkAndroidUpdate(): Promise<AndroidUpdateStatus> {
  return invoke<AndroidUpdateStatus>('android_update_check');
}

/**
 * Download the APK into the app cache and verify its SHA-256.
 * Returns the verified local file path; rejects on any digest mismatch.
 */
export async function downloadAndroidUpdate(apkUrl: string, sha256: string): Promise<string> {
  return invoke<string>('android_update_download', { apkUrl, sha256 });
}

/** Hand the verified APK to Android's package installer (user-confirmed). */
export async function installAndroidUpdate(path: string): Promise<void> {
  return invoke('android_update_install', { path });
}
