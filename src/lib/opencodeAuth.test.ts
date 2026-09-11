import { describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  OPENCODE_GO_PROVIDER,
  OPENCODE_ZEN_PROVIDER,
  getOpenCodeAuthProviders,
  providerLabel,
  removeOpenCodeAuthProvider,
  setOpenCodeAuthProvider,
} from './opencodeAuth';

describe('getOpenCodeAuthProviders', () => {
  it('returns provider ids from the backend', async () => {
    mockIPC((cmd) => {
      expect(cmd).toBe('opencode_auth_status');
      return ['anthropic', OPENCODE_ZEN_PROVIDER];
    });
    await expect(getOpenCodeAuthProviders('srv-1')).resolves.toEqual([
      'anthropic',
      OPENCODE_ZEN_PROVIDER,
    ]);
  });

  it('propagates backend errors', async () => {
    mockIPC(() => {
      throw new Error('connection failed');
    });
    await expect(getOpenCodeAuthProviders('srv-1')).rejects.toThrow('connection failed');
  });
});

describe('setOpenCodeAuthProvider', () => {
  it('sends the key once over IPC and resolves', async () => {
    mockIPC((cmd, args) => {
      expect(cmd).toBe('opencode_auth_set_provider');
      expect(args).toEqual({
        serverId: 'srv-1',
        providerId: OPENCODE_GO_PROVIDER,
        apiKey: 'sk-test',
      });
      return null;
    });
    await expect(
      setOpenCodeAuthProvider('srv-1', OPENCODE_GO_PROVIDER, 'sk-test')
    ).resolves.toBeNull();
  });
});

describe('removeOpenCodeAuthProvider', () => {
  it('sends only the server and provider id (no secret)', async () => {
    mockIPC((cmd, args) => {
      expect(cmd).toBe('opencode_auth_remove_provider');
      expect(args).toEqual({ serverId: 'srv-1', providerId: 'openai' });
      return null;
    });
    await expect(removeOpenCodeAuthProvider('srv-1', 'openai')).resolves.toBeNull();
  });
});

describe('providerLabel', () => {
  it('labels known providers and falls back to the raw id', () => {
    expect(providerLabel(OPENCODE_ZEN_PROVIDER)).toBe('OpenCode Zen');
    expect(providerLabel(OPENCODE_GO_PROVIDER)).toBe('OpenCode Go');
    expect(providerLabel('openai')).toBe('openai');
  });
});
