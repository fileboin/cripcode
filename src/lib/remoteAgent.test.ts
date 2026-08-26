import { describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import { checkRemoteAgentInstalled } from './remoteAgent';

describe('checkRemoteAgentInstalled', () => {
  it('returns installed=true when agent is found', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'check_remote_agent_installed') {
        expect(args).toMatchObject({ serverId: 'server-1', binaryName: 'claude' });
        return {
          installed: true,
          path: '/usr/local/bin/claude',
          binaryName: 'claude',
          error: null,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const status = await checkRemoteAgentInstalled('server-1', 'claude');
    expect(status.installed).toBe(true);
    expect(status.path).toBe('/usr/local/bin/claude');
  });

  it('returns installed=false when agent is not found', async () => {
    mockIPC((cmd) => {
      if (cmd === 'check_remote_agent_installed') {
        return {
          installed: false,
          path: null,
          binaryName: 'codex',
          error: "Agent 'codex' not found on VPS",
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const status = await checkRemoteAgentInstalled('server-1', 'codex');
    expect(status.installed).toBe(false);
    expect(status.path).toBeNull();
  });

  it('rejects empty binary name', async () => {
    mockIPC(() => {
      throw new Error('Validation failed for `binary_name`: Binary name must not be empty');
    });
    await expect(checkRemoteAgentInstalled('server-1', '')).rejects.toThrow();
  });
});
