import { describe, expect, it } from 'vitest';
import { mockIPC } from '@tauri-apps/api/mocks';
import {
  listSshServers,
  addSshServer,
  updateSshServer,
  deleteSshServer,
  testSshConnection,
  connectSsh,
  disconnectSsh,
  getSshConnectionState,
  buildSshTerminalArgs,
  type SshServer,
} from './ssh';

const sampleServer = {
  id: 'abc-123',
  name: 'Production VPS',
  host: 'example.com',
  port: 22,
  username: 'deploy',
  keyPath: '/Users/me/.ssh/id_ed25519',
  createdAt: 1724600000000,
  lastConnectedAt: null,
};

describe('listSshServers', () => {
  it('returns the server list from the backend', async () => {
    mockIPC((cmd) => {
      if (cmd === 'list_ssh_servers') return [sampleServer];
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(listSshServers()).resolves.toEqual([sampleServer]);
  });

  it('returns an empty array when no servers are configured', async () => {
    mockIPC((cmd) => {
      if (cmd === 'list_ssh_servers') return [];
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(listSshServers()).resolves.toEqual([]);
  });
});

describe('addSshServer', () => {
  it('calls add_ssh_server with the config fields', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'add_ssh_server') {
        expect(args).toMatchObject({
          name: 'My VPS',
          host: '203.0.113.1',
          username: 'root',
        });
        return { ...sampleServer, name: 'My VPS', host: '203.0.113.1' };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const result = await addSshServer({
      name: 'My VPS',
      host: '203.0.113.1',
      port: 2222,
      username: 'root',
      keyPath: '/home/me/.ssh/id_rsa',
    });
    expect(result.name).toBe('My VPS');
  });
});

describe('updateSshServer', () => {
  it('calls update_ssh_server with the id and config', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'update_ssh_server') {
        expect(args).toMatchObject({ id: 'abc-123', name: 'Renamed' });
        return { ...sampleServer, name: 'Renamed' };
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    const result = await updateSshServer('abc-123', {
      name: 'Renamed',
      host: 'example.com',
      port: 22,
      username: 'deploy',
      keyPath: null,
    });
    expect(result.name).toBe('Renamed');
  });
});

describe('deleteSshServer', () => {
  it('calls delete_ssh_server with the id', async () => {
    mockIPC((cmd, args) => {
      if (cmd === 'delete_ssh_server') {
        expect(args).toMatchObject({ id: 'abc-123' });
        return null;
      }
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(deleteSshServer('abc-123')).resolves.toBeNull();
  });
});

describe('testSshConnection', () => {
  it('returns "ok" when the connection succeeds', async () => {
    mockIPC((cmd) => {
      if (cmd === 'test_ssh_connection') return 'ok';
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(testSshConnection('abc-123')).resolves.toBe('ok');
  });
});

describe('connectSsh', () => {
  it('resolves when connect_ssh succeeds', async () => {
    mockIPC((cmd) => {
      if (cmd === 'connect_ssh') return null;
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(connectSsh('abc-123')).resolves.toBeNull();
  });
});

describe('disconnectSsh', () => {
  it('resolves when disconnect_ssh succeeds', async () => {
    mockIPC((cmd) => {
      if (cmd === 'disconnect_ssh') return null;
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(disconnectSsh('abc-123')).resolves.toBeNull();
  });
});

describe('getSshConnectionState', () => {
  it('returns the connection state from the backend', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_ssh_connection_state') return 'connected';
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(getSshConnectionState('abc-123')).resolves.toBe('connected');
  });

  it('returns disconnected as default', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_ssh_connection_state') return 'disconnected';
      throw new Error(`unexpected command: ${cmd}`);
    });
    await expect(getSshConnectionState('unknown-id')).resolves.toBe('disconnected');
  });
});

describe('buildSshTerminalArgs', () => {
  const server: SshServer = {
    id: 'abc',
    name: 'Test',
    host: 'example.com',
    port: 2222,
    username: 'deploy',
    keyPath: '/home/me/.ssh/id_ed25519',
    createdAt: 0,
    lastConnectedAt: null,
  };

  it('includes keepalive and host-key options', () => {
    const args = buildSshTerminalArgs(server);
    const joined = args.join(' ');
    expect(joined).toContain('ServerAliveInterval=30');
    expect(joined).toContain('ServerAliveCountMax=3');
    expect(joined).toContain('accept-new');
    expect(joined).toContain('ConnectTimeout=10');
  });

  it('forces pseudo-terminal allocation with -t', () => {
    const args = buildSshTerminalArgs(server);
    expect(args).toContain('-t');
  });

  it('includes port when not 22', () => {
    const args = buildSshTerminalArgs(server);
    expect(args).toContain('-p');
    expect(args).toContain('2222');
  });

  it('omits port when 22', () => {
    const args = buildSshTerminalArgs({ ...server, port: 22 });
    expect(args).not.toContain('-p');
  });

  it('includes key path when set', () => {
    const args = buildSshTerminalArgs(server);
    expect(args).toContain('-i');
    expect(args).toContain('/home/me/.ssh/id_ed25519');
  });

  it('omits key path when null', () => {
    const args = buildSshTerminalArgs({ ...server, keyPath: null });
    expect(args).not.toContain('-i');
  });

  it('ends with user@host', () => {
    const args = buildSshTerminalArgs(server);
    expect(args[args.length - 1]).toBe('deploy@example.com');
  });
});
