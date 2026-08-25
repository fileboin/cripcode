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
