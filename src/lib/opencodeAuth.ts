/**
 * OpenCode provider credentials on a remote server.
 *
 * Thin wrappers around the Rust backend commands that read/merge/remove API
 * keys in the remote OpenCode auth store (`~/.local/share/opencode/auth.json`).
 * Keys travel over IPC once and are written via SSH stdin; status checks
 * return provider ids only — never credential values. Any provider id OpenCode
 * understands can be managed (Zen and Go are just the well-known defaults).
 *
 * @module lib/opencodeAuth
 */

import { invoke } from '@tauri-apps/api/core';

/** Provider id used by OpenCode for OpenCode Zen. */
export const OPENCODE_ZEN_PROVIDER = 'opencode';
/** Provider id used by OpenCode for OpenCode Go. */
export const OPENCODE_GO_PROVIDER = 'opencode-go';

/** Friendly labels for the providers Cripcode surfaces by default. */
const KNOWN_PROVIDER_LABELS: Record<string, string> = {
  [OPENCODE_ZEN_PROVIDER]: 'OpenCode Zen',
  [OPENCODE_GO_PROVIDER]: 'OpenCode Go',
};

/** Display label for a provider id (falls back to the id itself). */
export function providerLabel(providerId: string): string {
  return KNOWN_PROVIDER_LABELS[providerId] ?? providerId;
}

/** Provider ids currently present in the remote OpenCode auth store. */
export async function getOpenCodeAuthProviders(serverId: string): Promise<string[]> {
  return invoke<string[]>('opencode_auth_status', { serverId });
}

/** Merge one provider's API key into the remote OpenCode auth store. */
export async function setOpenCodeAuthProvider(
  serverId: string,
  providerId: string,
  apiKey: string
): Promise<void> {
  return invoke<void>('opencode_auth_set_provider', { serverId, providerId, apiKey });
}

/** Remove one provider's API key from the remote OpenCode auth store. */
export async function removeOpenCodeAuthProvider(
  serverId: string,
  providerId: string
): Promise<void> {
  return invoke<void>('opencode_auth_remove_provider', { serverId, providerId });
}
