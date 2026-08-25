/**
 * Ollama connection and model discovery.
 *
 * Wraps the Rust backend commands for checking Ollama status (local or
 * remote via SSH) and listing available models.
 *
 * @module lib/ollama
 */

import { invoke } from '@tauri-apps/api/core';

export interface OllamaModelDetails {
  family: string;
  parameterSize: string;
  quantizationLevel: string | null;
}

export interface OllamaModel {
  name: string;
  model: string;
  size: number;
  quantization: string | null;
  details: OllamaModelDetails | null;
}

export interface OllamaStatus {
  installed: boolean;
  running: boolean;
  version: string | null;
  endpoint: string;
  error: string | null;
}

/**
 * Check Ollama status. When `serverId` is null, checks locally.
 * When set, checks on the remote VPS via SSH exec.
 */
export async function checkOllamaStatus(serverId: string | null): Promise<OllamaStatus> {
  return invoke<OllamaStatus>('check_ollama_status', { serverId });
}

/**
 * List available Ollama models. When `serverId` is null, queries the local
 * Ollama API directly. When set, uses SSH exec + curl on the VPS.
 */
export async function listOllamaModels(serverId: string | null): Promise<OllamaModel[]> {
  const models = await invoke<
    Array<{
      name: string;
      model: string;
      size: number;
      quantization_level: string | null;
      details: {
        family: string;
        parameter_size: string;
        quantization_level: string | null;
      } | null;
    }>
  >('list_ollama_models', { serverId });

  return models.map((m) => ({
    name: m.name,
    model: m.model,
    size: m.size,
    quantization: m.quantization_level,
    details: m.details
      ? {
          family: m.details.family,
          parameterSize: m.details.parameter_size,
          quantizationLevel: m.details.quantization_level,
        }
      : null,
  }));
}

/** Format a model size in bytes to a human-readable string. */
export function formatModelSize(bytes: number): string {
  if (bytes < 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(0)} MB`;
  }
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

/** Detailed info for a single model (from /api/show). */
export interface OllamaModelInfo {
  name: string;
  family: string;
  parameterSize: string;
  quantization: string | null;
  contextLength: number | null;
  parameterCount: number | null;
  loaded: boolean;
}

/** Format a context length in tokens to a human-readable string. */
export function formatContextLength(tokens: number | null): string {
  if (tokens === null) return 'unknown';
  if (tokens >= 1000) return `${(tokens / 1000).toFixed(0)}K`;
  return `${tokens}`;
}

/**
 * Get detailed info for a single model via Ollama's `/api/show` endpoint.
 * Provides context window length and other details not available from `/api/tags`.
 */
export async function getOllamaModelInfo(
  serverId: string | null,
  modelName: string
): Promise<OllamaModelInfo> {
  const result = await invoke<{
    name: string;
    family: string;
    parameter_size: string;
    quantization: string | null;
    context_length: number | null;
    parameter_count: number | null;
    loaded: boolean;
  }>('get_ollama_model_info', { serverId, modelName });

  return {
    name: result.name,
    family: result.family,
    parameterSize: result.parameter_size,
    quantization: result.quantization,
    contextLength: result.context_length,
    parameterCount: result.parameter_count,
    loaded: result.loaded,
  };
}

// ============ Model Selection ============

/**
 * Get the currently selected Ollama model for a location.
 * Returns null if no model has been selected (the app should use a default).
 */
export async function getSelectedOllamaModel(serverId: string | null): Promise<string | null> {
  return invoke<string | null>('get_selected_ollama_model', { serverId });
}

/**
 * Set the selected Ollama model for a location.
 * Persists to disk so the selection survives app restarts.
 */
export async function setSelectedOllamaModel(
  serverId: string | null,
  modelName: string
): Promise<void> {
  await invoke('set_selected_ollama_model', { serverId, modelName });
}

/**
 * Clear the selected Ollama model for a location (reset to default).
 */
export async function clearSelectedOllamaModel(serverId: string | null): Promise<void> {
  await invoke('clear_selected_ollama_model', { serverId });
}
