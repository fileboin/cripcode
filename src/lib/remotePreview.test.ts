import { describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  startRemotePreviewTunnel,
  stopRemotePreviewTunnel,
  getRemotePreviewStatus,
} from './remotePreview';

describe('startRemotePreviewTunnel', () => {
  it('returns the local port', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'start_remote_preview_tunnel') {
        expect(args).toMatchObject({ serverId: 'server-1', remotePort: 3000 });
        return 13000;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(startRemotePreviewTunnel('server-1', 3000)).resolves.toBe(13000);
  });

  it('passes localPort when specified', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'start_remote_preview_tunnel') {
        expect(args).toMatchObject({ localPort: 8080 });
        return 8080;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(startRemotePreviewTunnel('server-1', 3000, 8080)).resolves.toBe(8080);
  });
});

describe('stopRemotePreviewTunnel', () => {
  it('calls stop_remote_preview_tunnel', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'stop_remote_preview_tunnel') {
        expect(args).toMatchObject({ serverId: 'server-1', remotePort: 3000 });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(stopRemotePreviewTunnel('server-1', 3000)).resolves.toBeUndefined();
  });
});

describe('getRemotePreviewStatus', () => {
  it('returns live status when server is responding', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_remote_preview_status') {
        return {
          tunnelActive: true,
          localPort: 13000,
          remotePort: 3000,
          serverResponding: true,
          httpStatus: 200,
          error: null,
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const status = await getRemotePreviewStatus('server-1', 3000);
    expect(status.tunnelActive).toBe(true);
    expect(status.serverResponding).toBe(true);
    expect(status.httpStatus).toBe(200);
  });

  it('returns tunnel-up-but-no-server status', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_remote_preview_status') {
        return {
          tunnelActive: true,
          localPort: 13000,
          remotePort: 3000,
          serverResponding: false,
          httpStatus: null,
          error: 'Tunnel active but remote dev server not responding',
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const status = await getRemotePreviewStatus('server-1', 3000);
    expect(status.tunnelActive).toBe(true);
    expect(status.serverResponding).toBe(false);
  });

  it('returns no-tunnel status', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_remote_preview_status') {
        return {
          tunnelActive: false,
          localPort: null,
          remotePort: null,
          serverResponding: false,
          httpStatus: null,
          error: 'No active tunnel for this server/port',
        };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const status = await getRemotePreviewStatus('server-1', 3000);
    expect(status.tunnelActive).toBe(false);
    expect(status.localPort).toBeNull();
  });
});
