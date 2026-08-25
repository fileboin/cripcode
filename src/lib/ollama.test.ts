import { describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import { checkOllamaStatus, listOllamaModels, formatModelSize } from './ollama';

describe('checkOllamaStatus', () => {
  it('returns status for local Ollama (running)', async () => {
    mockIPC((cmd) => {
      if (cmd === 'check_ollama_status') {
        return {
          installed: true,
          running: true,
          version: '0.1.0',
          endpoint: 'http://localhost:11434',
          error: null,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const status = await checkOllamaStatus(null);
    expect(status.installed).toBe(true);
    expect(status.running).toBe(true);
    expect(status.version).toBe('0.1.0');
  });

  it('returns status for local Ollama (not installed)', async () => {
    mockIPC((cmd) => {
      if (cmd === 'check_ollama_status') {
        return {
          installed: false,
          running: false,
          version: null,
          endpoint: 'http://localhost:11434',
          error: 'Ollama binary not found on PATH',
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const status = await checkOllamaStatus(null);
    expect(status.installed).toBe(false);
    expect(status.running).toBe(false);
  });

  it('passes serverId for remote check', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'check_ollama_status') {
        expect(args).toMatchObject({ serverId: 'server-1' });
        return {
          installed: true,
          running: true,
          version: '0.2.0',
          endpoint: 'ssh://user@host:22/ollama',
          error: null,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const status = await checkOllamaStatus('server-1');
    expect(status.running).toBe(true);
    expect(status.version).toBe('0.2.0');
  });
});

describe('listOllamaModels', () => {
  it('returns models from local Ollama', async () => {
    mockIPC((cmd) => {
      if (cmd === 'list_ollama_models') {
        return [
          {
            name: 'llama3:latest',
            model: 'llama3:latest',
            size: 3825829519,
            quantization_level: 'q4_K_M',
            details: {
              family: 'llama',
              parameter_size: '8B',
              quantization_level: 'q4_K_M',
            },
          },
        ];
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const models = await listOllamaModels(null);
    expect(models).toHaveLength(1);
    expect(models[0].name).toBe('llama3:latest');
    expect(models[0].quantization).toBe('q4_K_M');
    expect(models[0].details?.parameterSize).toBe('8B');
  });

  it('returns empty array when no models', async () => {
    mockIPC((cmd) => {
      if (cmd === 'list_ollama_models') return [];
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(listOllamaModels(null)).resolves.toEqual([]);
  });

  it('passes serverId for remote models', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'list_ollama_models') {
        expect(args).toMatchObject({ serverId: 'server-1' });
        return [];
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(listOllamaModels('server-1')).resolves.toEqual([]);
  });
});

describe('formatModelSize', () => {
  it('formats bytes as MB', () => {
    expect(formatModelSize(500 * 1024 * 1024)).toBe('500 MB');
  });

  it('formats bytes as GB', () => {
    expect(formatModelSize(4_700_000_000)).toBe('4.4 GB');
  });

  it('formats exactly 1 GB', () => {
    expect(formatModelSize(1024 * 1024 * 1024)).toBe('1.0 GB');
  });
});
