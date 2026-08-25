import { describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  checkOllamaStatus,
  listOllamaModels,
  getOllamaModelInfo,
  getSelectedOllamaModel,
  setSelectedOllamaModel,
  clearSelectedOllamaModel,
  formatModelSize,
  formatContextLength,
} from './ollama';

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

describe('getOllamaModelInfo', () => {
  it('returns model info with context length', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'get_ollama_model_info') {
        expect(args).toMatchObject({ serverId: null, modelName: 'llama3:latest' });
        return {
          name: 'llama3:latest',
          family: 'llama',
          parameter_size: '8B',
          quantization: 'q4_K_M',
          context_length: 8192,
          parameter_count: 8030261248,
          loaded: false,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const info = await getOllamaModelInfo(null, 'llama3:latest');
    expect(info.name).toBe('llama3:latest');
    expect(info.family).toBe('llama');
    expect(info.contextLength).toBe(8192);
    expect(info.parameterSize).toBe('8B');
    expect(info.quantization).toBe('q4_K_M');
  });

  it('passes serverId for remote model info', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'get_ollama_model_info') {
        expect(args).toMatchObject({ serverId: 'server-1', modelName: 'gemma:latest' });
        return {
          name: 'gemma:latest',
          family: 'gemma',
          parameter_size: '7B',
          quantization: null,
          context_length: 4096,
          parameter_count: null,
          loaded: false,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const info = await getOllamaModelInfo('server-1', 'gemma:latest');
    expect(info.family).toBe('gemma');
    expect(info.contextLength).toBe(4096);
    expect(info.quantization).toBeNull();
  });

  it('handles null context length', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_ollama_model_info') {
        return {
          name: 'custom:latest',
          family: 'unknown',
          parameter_size: '?',
          quantization: null,
          context_length: null,
          parameter_count: null,
          loaded: false,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const info = await getOllamaModelInfo(null, 'custom:latest');
    expect(info.contextLength).toBeNull();
  });
});

describe('formatContextLength', () => {
  it('formats large token counts as K', () => {
    expect(formatContextLength(8192)).toBe('8K');
    expect(formatContextLength(128000)).toBe('128K');
  });

  it('formats small token counts as-is', () => {
    expect(formatContextLength(512)).toBe('512');
  });

  it('returns unknown for null', () => {
    expect(formatContextLength(null)).toBe('unknown');
  });
});

describe('getSelectedOllamaModel', () => {
  it('returns the selected model for local', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'get_selected_ollama_model') {
        expect(args).toMatchObject({ serverId: null });
        return 'llama3:latest';
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(getSelectedOllamaModel(null)).resolves.toBe('llama3:latest');
  });

  it('returns null when no model selected', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_selected_ollama_model') return null;
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(getSelectedOllamaModel(null)).resolves.toBeNull();
  });

  it('passes serverId for remote', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'get_selected_ollama_model') {
        expect(args).toMatchObject({ serverId: 'server-1' });
        return 'gemma:latest';
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(getSelectedOllamaModel('server-1')).resolves.toBe('gemma:latest');
  });
});

describe('setSelectedOllamaModel', () => {
  it('calls set_selected_ollama_model with the right args', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'set_selected_ollama_model') {
        expect(args).toMatchObject({ serverId: null, modelName: 'llama3:latest' });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(setSelectedOllamaModel(null, 'llama3:latest')).resolves.toBeUndefined();
  });

  it('passes serverId for remote', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'set_selected_ollama_model') {
        expect(args).toMatchObject({ serverId: 'server-1', modelName: 'gemma:latest' });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(setSelectedOllamaModel('server-1', 'gemma:latest')).resolves.toBeUndefined();
  });
});

describe('clearSelectedOllamaModel', () => {
  it('calls clear_selected_ollama_model', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'clear_selected_ollama_model') {
        expect(args).toMatchObject({ serverId: null });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(clearSelectedOllamaModel(null)).resolves.toBeUndefined();
  });
});
