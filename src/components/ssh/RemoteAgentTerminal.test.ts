import { describe, expect, it } from 'vitest';
import { ALL_AGENTS } from '../../lib/agent';
import type { SshServer } from '../../lib/ssh';
import { buildRemoteAgentSpawn } from './RemoteAgentTerminal';

const server: SshServer = {
  id: 'srv-1',
  name: 'hetzner',
  host: '91.99.126.64',
  port: 22,
  username: 'root',
  keyPath: null,
  authType: 'password',
  createdAt: 0,
  lastConnectedAt: null,
};

const opencode = ALL_AGENTS.find((agent) => agent.id === 'opencode')!;

describe('buildRemoteAgentSpawn', () => {
  it('sends the backend execution target when a remote project is bound', () => {
    const spawn = buildRemoteAgentSpawn(server, opencode, '/root/ollama', 'proj-1');

    // The backend builds ssh argv + askpass wiring from the target — the
    // frontend must not ship its own raw ssh args on this path.
    expect(spawn.executionTarget).toEqual({
      kind: 'ssh',
      serverId: 'srv-1',
      remoteProjectId: 'proj-1',
      remotePath: '/root/ollama',
    });
    expect(spawn.command).toBe(opencode.binaryName);
    expect(spawn.args).toEqual([]);
    expect(spawn.sshServerId).toBeUndefined();
  });

  it('keeps the legacy raw-ssh spawn when there is no registered project', () => {
    const spawn = buildRemoteAgentSpawn(server, opencode, '/root/ollama');

    expect(spawn.executionTarget).toBeUndefined();
    expect(spawn.command).toBe('ssh');
    expect(spawn.args[spawn.args.length - 1]).toBe("cd '/root/ollama' && 'opencode'");
    expect(spawn.args.join(' ')).toContain('root@91.99.126.64');
    // The legacy raw-ssh fallback is gated, so it must fail closed on an
    // unknown host instead of silently trusting it.
    expect(spawn.args.join(' ')).toContain('StrictHostKeyChecking=yes');
    expect(spawn.args.join(' ')).not.toContain('accept-new');
    // It still names the server so the backend can wire the keystore askpass
    // channel (id only — no secret crosses the boundary).
    expect(spawn.sshServerId).toBe('srv-1');
  });
});
