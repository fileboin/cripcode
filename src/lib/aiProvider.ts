/**
 * AI Provider Abstraction
 *
 * Unifies existing CLI agents (Claude Code, Codex, OpenCode, Cursor) with
 * API-based providers like Ollama. The existing agent spawning (PTY/terminal)
 * is NOT changed — this is a read-only registry for provider discovery.
 *
 * @module lib/aiProvider
 */

import { invoke } from '@tauri-apps/api/core';

/** The type of an AI provider — determines how it's accessed. */
export type ProviderType = 'cli' | 'ollama';

/** Information about an AI provider. */
export interface ProviderInfo {
  /** Unique identifier (e.g. "claude-code", "ollama"). */
  id: string;
  /** Human-readable name (e.g. "Claude Code", "Ollama"). */
  name: string;
  /** Provider type — CLI (terminal) or Ollama (API). */
  providerType: ProviderType;
  /** Whether the provider is available (binary found, or API running). */
  available: boolean;
  /** Short description for UI display. */
  description: string;
  /** For Ollama: the SSH server ID if remote, null if local. For CLI: always null. */
  serverId: string | null;
}

/** List all available AI providers (CLI agents + Ollama local/remote). */
export async function listAiProviders(): Promise<ProviderInfo[]> {
  return invoke<ProviderInfo[]>('list_ai_providers');
}

/** Get info for a single provider by ID. */
export async function getAiProviderInfo(providerId: string): Promise<ProviderInfo> {
  return invoke<ProviderInfo>('get_ai_provider_info', { providerId });
}
