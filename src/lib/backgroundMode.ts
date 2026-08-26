/**
 * Background mode — prevents system sleep when active tasks are running.
 *
 * Three modes: "off", "smart" (default), "always_on".
 * The frontend reports active task counts; SMART mode prevents sleep
 * when count > 0.
 *
 * @module lib/backgroundMode
 */

import { invoke } from '@tauri-apps/api/core';

export type BackgroundMode = 'off' | 'smart' | 'always_on';

/** Get the current background mode. */
export async function getBackgroundMode(): Promise<BackgroundMode> {
  return invoke<string>('get_background_mode') as Promise<BackgroundMode>;
}

/** Set the background mode. Persists across restarts. */
export async function setBackgroundMode(mode: BackgroundMode): Promise<void> {
  await invoke('set_background_mode', { mode });
}

/** Report the number of active tasks (dev servers, builds, tunnels, agents). */
export async function reportActiveTaskCount(count: number): Promise<void> {
  await invoke('report_active_task_count', { count });
}

/** Check if the system is currently prevented from sleeping. */
export async function isPreventingSleep(): Promise<boolean> {
  return invoke<boolean>('is_preventing_sleep');
}
