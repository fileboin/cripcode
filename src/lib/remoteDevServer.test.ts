import { describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  startRemoteDevServer,
  stopRemoteDevServer,
  restartRemoteDevServer,
  getRemoteDevServerStatus,
  getRemoteDevServerLogs,
} from './remoteDevServer';

describe('startRemoteDevServer', () => {
  it('calls start_remote_dev_server with the right args', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'start_remote_dev_server') {
        expect(args).toMatchObject({
          serverId: 'server-1',
          remotePath: '/home/user/app',
          command: 'npm run dev',
          port: 3000,
        });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(
      startRemoteDevServer('server-1', '/home/user/app', 'npm run dev', 3000)
    ).resolves.toBeUndefined();
  });

  it('passes null port when not specified', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'start_remote_dev_server') {
        expect(args).toMatchObject({ port: null });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(
      startRemoteDevServer('server-1', '/home/user/app', 'npm run dev', null)
    ).resolves.toBeUndefined();
  });
});

describe('stopRemoteDevServer', () => {
  it('calls stop_remote_dev_server', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'stop_remote_dev_server') {
        expect(args).toMatchObject({ serverId: 'server-1', remotePath: '/home/user/app' });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(stopRemoteDevServer('server-1', '/home/user/app')).resolves.toBeUndefined();
  });
});

describe('restartRemoteDevServer', () => {
  it('calls restart_remote_dev_server', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'restart_remote_dev_server') {
        expect(args).toMatchObject({ command: 'npm run dev', port: 3000 });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(
      restartRemoteDevServer('server-1', '/home/user/app', 'npm run dev', 3000)
    ).resolves.toBeUndefined();
  });
});

describe('getRemoteDevServerStatus', () => {
  it('returns running status', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_remote_dev_server_status') {
        return {
          running: true,
          pid: 12345,
          port: 3000,
          logLines: 42,
          error: null,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const status = await getRemoteDevServerStatus('server-1', '/home/user/app');
    expect(status.running).toBe(true);
    expect(status.pid).toBe(12345);
    expect(status.logLines).toBe(42);
  });

  it('returns stopped status', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_remote_dev_server_status') {
        return {
          running: false,
          pid: null,
          port: null,
          logLines: 0,
          error: null,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const status = await getRemoteDevServerStatus('server-1', '/home/user/app');
    expect(status.running).toBe(false);
  });
});

describe('getRemoteDevServerLogs', () => {
  it('returns log text', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_remote_dev_server_logs') {
        return 'Server started on port 3000\nReady in 1.2s';
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const logs = await getRemoteDevServerLogs('server-1', '/home/user/app', 50);
    expect(logs).toContain('Server started');
  });

  it('passes null lines by default', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'get_remote_dev_server_logs') {
        expect(args).toMatchObject({ lines: null });
        return '';
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(getRemoteDevServerLogs('server-1', '/home/user/app')).resolves.toBe('');
  });
});
