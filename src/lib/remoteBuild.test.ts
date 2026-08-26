import { describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  startRemoteBuild,
  stopRemoteBuild,
  getRemoteBuildStatus,
  getRemoteBuildLogs,
} from './remoteBuild';

describe('startRemoteBuild', () => {
  it('calls start_remote_build with the right args', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'start_remote_build') {
        expect(args).toMatchObject({
          serverId: 'server-1',
          remotePath: '/home/user/app',
          command: 'npm run build',
        });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(
      startRemoteBuild('server-1', '/home/user/app', 'npm run build')
    ).resolves.toBeUndefined();
  });
});

describe('stopRemoteBuild', () => {
  it('calls stop_remote_build', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'stop_remote_build') {
        expect(args).toMatchObject({ serverId: 'server-1', remotePath: '/home/user/app' });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(
      stopRemoteBuild('server-1', '/home/user/app')
    ).resolves.toBeUndefined();
  });
});

describe('getRemoteBuildStatus', () => {
  it('returns running status', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_remote_build_status') {
        return {
          running: true,
          exitCode: null,
          success: null,
          logLines: 42,
          error: null,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const status = await getRemoteBuildStatus('server-1', '/home/user/app');
    expect(status.running).toBe(true);
    expect(status.exitCode).toBeNull();
    expect(status.logLines).toBe(42);
  });

  it('returns success status', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_remote_build_status') {
        return {
          running: false,
          exitCode: 0,
          success: true,
          logLines: 100,
          error: null,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const status = await getRemoteBuildStatus('server-1', '/home/user/app');
    expect(status.running).toBe(false);
    expect(status.success).toBe(true);
    expect(status.exitCode).toBe(0);
  });

  it('returns failure status', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_remote_build_status') {
        return {
          running: false,
          exitCode: 1,
          success: false,
          logLines: 50,
          error: null,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const status = await getRemoteBuildStatus('server-1', '/home/user/app');
    expect(status.running).toBe(false);
    expect(status.success).toBe(false);
    expect(status.exitCode).toBe(1);
  });
});

describe('getRemoteBuildLogs', () => {
  it('returns log text', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_remote_build_logs') {
        return 'Building...\nDone.';
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const logs = await getRemoteBuildLogs('server-1', '/home/user/app', 200);
    expect(logs).toContain('Building');
  });

  it('passes null lines by default', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'get_remote_build_logs') {
        expect(args).toMatchObject({ lines: null });
        return '';
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(
      getRemoteBuildLogs('server-1', '/home/user/app')
    ).resolves.toBe('');
  });
});
