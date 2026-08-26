import { describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import { listAiProviders, getAiProviderInfo } from './aiProvider';

describe('listAiProviders', () => {
  it('returns the provider list from the backend', async () => {
    mockIPC((cmd) => {
      if (cmd === 'list_ai_providers') {
        return [
          {
            id: 'claude-code',
            name: 'Claude Code',
            providerType: 'cli',
            available: true,
            description: 'Claude Code CLI agent',
            serverId: null,
          },
          {
            id: 'ollama',
            name: 'Ollama',
            providerType: 'ollama',
            available: true,
            description: 'Local Ollama instance (API on port 11434)',
            serverId: null,
          },
        ];
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const providers = await listAiProviders();
    expect(providers).toHaveLength(2);
    expect(providers[0].id).toBe('claude-code');
    expect(providers[0].providerType).toBe('cli');
    expect(providers[1].id).toBe('ollama');
    expect(providers[1].providerType).toBe('ollama');
  });

  it('returns an empty array when no providers', async () => {
    mockIPC((cmd) => {
      if (cmd === 'list_ai_providers') return [];
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(listAiProviders()).resolves.toEqual([]);
  });
});

describe('getAiProviderInfo', () => {
  it('returns info for a specific provider', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'get_ai_provider_info') {
        expect(args).toMatchObject({ providerId: 'ollama' });
        return {
          id: 'ollama',
          name: 'Ollama',
          providerType: 'ollama',
          available: true,
          description: 'Local Ollama instance',
          serverId: null,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const info = await getAiProviderInfo('ollama');
    expect(info.id).toBe('ollama');
    expect(info.providerType).toBe('ollama');
    expect(info.available).toBe(true);
  });

  it('throws when provider not found', async () => {
    mockIPC(() => {
      throw new Error('No AI provider found with id `nonexistent`');
    });
    await expect(getAiProviderInfo('nonexistent')).rejects.toThrow();
  });
});
